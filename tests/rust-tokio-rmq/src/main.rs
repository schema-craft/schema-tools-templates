//! Integration tests for the rust/rabbitmq-tokio templates. They need a RabbitMQ broker:
//! `AMQP_URI` (default `amqp://guest:guest@localhost:5672/%2f`), e.g.
//! `docker run -d -p 5672:5672 rabbitmq:4 && cargo test`.
#[rustfmt::skip]
#[allow(dead_code, unused_imports)]
mod rabbitmq;

fn main() {}

#[cfg(test)]
mod tests {
    use crate::rabbitmq::message::{DeviceCreatedV1, DeviceCreatedV1Meta, DeviceCreatedV1Payload};
    use crate::rabbitmq::*;
    use lapin::options::{QueueDeclareOptions, QueueDeleteOptions};
    use lapin::types::{AMQPValue, FieldTable, LongString};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, Once};
    use std::time::Duration;

    fn manager(name: &str) -> Arc<RabbitConnectionManager> {
        let uri = std::env::var("AMQP_URI")
            .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".into());
        Arc::new(RabbitConnectionManager::new(RabbitConfiguration {
            uri,
            connection_name: name.into(),
        }))
    }

    /// Recreates `queue` so each test starts empty. `args` = extra queue arguments.
    async fn fresh_queue(m: &RabbitConnectionManager, queue: &str, args: FieldTable) {
        let ch = m.get().await.unwrap().create_channel().await.unwrap();
        ch.queue_delete(queue.into(), QueueDeleteOptions::default()).await.unwrap();
        let opts = QueueDeclareOptions { durable: true, ..Default::default() };
        ch.queue_declare(queue.into(), opts, args).await.unwrap();
        ch.close(200, "ok".into()).await.unwrap();
    }

    async fn ready_count(m: &RabbitConnectionManager, queue: &str) -> u32 {
        let ch = m.get().await.unwrap().create_channel().await.unwrap();
        let opts = QueueDeclareOptions { passive: true, ..Default::default() };
        let q = ch.queue_declare(queue.into(), opts, FieldTable::default()).await.unwrap();
        ch.close(200, "ok".into()).await.unwrap();
        q.message_count()
    }

    fn device(id: &str) -> DeviceCreatedV1 {
        DeviceCreatedV1::new(
            DeviceCreatedV1Meta::new(None, None),
            DeviceCreatedV1Payload::new(id.into()),
        )
    }

    /// A broker nack (queue full with reject-publish) must be an error, never a silent `Ok`.
    #[tokio::test]
    async fn broker_nack_is_an_error() {
        let m = manager("test-nack");
        let mut args = FieldTable::default();
        args.insert("x-max-length".into(), AMQPValue::LongInt(1));
        args.insert("x-overflow".into(), AMQPValue::LongString(LongString::from("reject-publish")));
        fresh_queue(&m, "tokio_rmq_nack", args).await;
        fresh_queue(&m, "tokio_rmq_nack_ok", FieldTable::default()).await;
        let p = RabbitProducer::new(m.clone(), "").await.unwrap();

        p.publish("tokio_rmq_nack", device("1")).await.unwrap();

        let err = p.publish("tokio_rmq_nack", device("2")).await.unwrap_err();
        assert!(err.to_string().contains("rejected 1 of 1"), "{err}");
        let err = p.publish_raw("tokio_rmq_nack", b"{}").await.unwrap_err();
        assert!(err.to_string().contains("rejected 1 of 1"), "{err}");
        let batch = vec![device("3"), device("4"), device("5")];
        let err = p.batch_publish("tokio_rmq_nack", batch).await.unwrap_err();
        assert!(err.to_string().contains("rejected 3 of 3"), "{err}");

        // a nack is not a channel failure: the same producer keeps working
        p.publish("tokio_rmq_nack_ok", device("6")).await.unwrap();
        p.batch_publish("tokio_rmq_nack_ok", vec![device("7"), device("8")]).await.unwrap();
        assert_eq!(ready_count(&m, "tokio_rmq_nack").await, 1);
        assert_eq!(ready_count(&m, "tokio_rmq_nack_ok").await, 3);

        p.close().await;
        m.close().await;
    }

    /// Shutdown while a message is in flight: it is finished and acked exactly once,
    /// nothing new starts, and the rest stays in the queue for the next consumer.
    #[tokio::test(flavor = "multi_thread")]
    async fn graceful_shutdown_acks_in_flight_message_once() {
        const QUEUE: &str = "tokio_rmq_shutdown";
        let m = manager("test-shutdown-setup");
        fresh_queue(&m, QUEUE, FieldTable::default()).await;
        let p = RabbitProducer::new(m.clone(), "").await.unwrap();
        for id in ["1", "2", "3"] {
            p.publish(QUEUE, device(id)).await.unwrap();
        }

        let run = |handler_ms: u64, seen: Arc<Mutex<Vec<String>>>| {
            RabbitConsumer::new(QUEUE, 1, (), Some(1), manager("test-shutdown"))
                .subscribe(move |msg: DeviceCreatedV1, _: ()| {
                    let seen = seen.clone();
                    async move {
                        seen.lock().unwrap().push(msg.payload.device_id);
                        tokio::time::sleep(Duration::from_millis(handler_ms)).await;
                    }
                })
        };

        let seen = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Shutdown::new();
        let handle = run(1000, seen.clone()).start(shutdown.child());
        while seen.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        shutdown.trigger();
        tokio::time::timeout(Duration::from_secs(10), handle).await.unwrap().unwrap();
        assert_eq!(*seen.lock().unwrap(), ["1"]);
        assert_eq!(ready_count(&m, QUEUE).await, 2);

        let seen = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Shutdown::new();
        let handle = run(0, seen.clone()).start(shutdown.child());
        tokio::time::timeout(Duration::from_secs(10), async {
            while seen.lock().unwrap().len() < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        shutdown.trigger();
        handle.await.unwrap();
        assert_eq!(*seen.lock().unwrap(), ["2", "3"], "message 1 must not be redelivered");

        p.close().await;
        m.close().await;
    }

    static LAPIN_PARKED: AtomicUsize = AtomicUsize::new(0);

    /// Counts lapin's trace event for a confirm parked on the channel because its publish
    /// was dropped halfway (it then stays there for the channel's whole life).
    struct ConfirmEvents;
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ConfirmEvents {
        fn on_event(&self, e: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            struct Msg(String);
            impl tracing::field::Visit for Msg {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    if f.name() == "message" {
                        self.0 = format!("{v:?}");
                    }
                }
            }
            let mut msg = Msg(String::new());
            e.record(&mut msg);
            if msg.0.contains("registering it for wait_for_confirms") {
                LAPIN_PARKED.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// A publish cancelled while waiting for its confirm (client disconnect, timeout) must
    /// not leave the confirm parked on the long-lived channel.
    #[tokio::test(flavor = "multi_thread")]
    async fn cancelled_publish_does_not_park_confirm() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            use tracing_subscriber::prelude::*;
            tracing_subscriber::registry().with(ConfirmEvents).init();
        });
        let m = manager("test-cancel");
        fresh_queue(&m, "tokio_rmq_cancel", FieldTable::default()).await;
        let p = RabbitProducer::new(m.clone(), "").await.unwrap();

        // A zero timeout drops the caller at its first pending point: inside lapin's
        // basic_publish (frame send) or while waiting for the confirm.
        const N: u32 = 50;
        let mut cancelled = 0;
        for _ in 0..N {
            let publish = p.publish("tokio_rmq_cancel", device("x"));
            if tokio::time::timeout(Duration::ZERO, publish).await.is_err() {
                cancelled += 1;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await; // let started publishes finish

        assert!(cancelled > 0, "no publish was cancelled, test proves nothing");
        assert_eq!(LAPIN_PARKED.load(Ordering::SeqCst), 0, "confirms parked on the channel");
        // a publish that has started always completes, even when its caller is gone
        assert_eq!(ready_count(&m, "tokio_rmq_cancel").await, N);

        p.close().await;
        m.close().await;
    }
}
