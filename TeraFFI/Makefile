SHELL := /bin/bash
.DEFAULT_GOAL := all
.SHELLFLAGS := -eu -o pipefail -c
.NOTPARALLEL:

.PHONY: all verify candidate-build candidate-check clean distclean

all: verify

verify:
	../scripts/ffi-artifacts.sh install

candidate-build:
	../scripts/ffi-artifacts.sh build

candidate-check:
	../scripts/ffi-artifacts.sh check

clean distclean:
	../scripts/ffi-artifacts.sh clean
