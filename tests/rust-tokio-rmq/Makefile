.PHONY: all install-dependencies clean

SCHEMATOOLS_VERSION := v1.1.0
SCHEMATOOLS_BIN = dependencies/schematools-$(SCHEMATOOLS_VERSION)
FORMAT =
SCHEMATOOLS_PLATFORM =

codegen: install-dependencies format
	@echo generating code ...

	$(SCHEMATOOLS_BIN) chain -vv \
		-c 'registry add templates ../../' \
		-c 'process dereference spec/message/all.yaml --skip-root-internal-references --create-internal-references' \
		-c 'codegen json-schema - --template templates::rust/_common/ --template templates::rust/rabbitmq-tokio/ -o apm=tracing-opentelemetry ${FORMAT} --target-dir src/'

install-dependencies: platform
	@echo installing dependencies ...

	mkdir -p dependencies/

	if [ ! -f ./dependencies/schematools-$(SCHEMATOOLS_VERSION) ]; then \
		curl -Lo dependencies/schematools.tar.gz https://github.com/kstasik/schema-tools/releases/download/$(SCHEMATOOLS_VERSION)/schematools-$(SCHEMATOOLS_PLATFORM).tar.gz; \
		tar -xvf dependencies/schematools.tar.gz -C dependencies/; \
		mv dependencies/schematools-$(SCHEMATOOLS_PLATFORM) dependencies/schematools-$(SCHEMATOOLS_VERSION); \
		chmod u+x dependencies/schematools-$(SCHEMATOOLS_VERSION); \
		rm dependencies/schematools.tar.gz; \
	fi

format:
	$(info Trying to find rustfmt)
ifneq (, $(shell which rustfmt))
    $(eval FORMAT := --format "rustfmt --edition 2021")
endif

platform:
	$(info Detecting platform)
    $(eval SCHEMATOOLS_PLATFORM := )

ifeq ($(OS),Windows_NT)
	$(error Windows not yet supported)
endif

    UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Linux)
    SCHEMATOOLS_PLATFORM = linux
else ifeq ($(UNAME_S),Darwin)
    SCHEMATOOLS_PLATFORM = darwin
else
    $(error Cannot detect os ${UNAME_S})
endif

    UNAME_M := $(shell uname -m)
ifeq ($(UNAME_M),x86_64)
	SCHEMATOOLS_PLATFORM := $(addsuffix -amd64,$(SCHEMATOOLS_PLATFORM))
else ifneq ($(filter arm%,$(UNAME_M)),)
	SCHEMATOOLS_PLATFORM := $(addsuffix -arm64,$(SCHEMATOOLS_PLATFORM))
else
	$(error Cannot detect architecture ${UNAME_M})
endif

    $(info Platform detected: ${SCHEMATOOLS_PLATFORM})

clean:
	@echo cleaning ...
