# Convenience targets for the C bindings. Pure Rust users just use `cargo`.
#
#   make capi      build libnoroi.so and libnoroi.a (release, capi feature)
#   make cdemo     build the C example in examples/demo.c
#   make clean     cargo clean

CARGO ?= cargo
CC    ?= cc
PROFILE ?= release
OUTDIR := target/$(PROFILE)

.PHONY: capi cdemo clean

capi:
	$(CARGO) rustc --$(PROFILE) --features capi --lib --crate-type cdylib
	$(CARGO) rustc --$(PROFILE) --features capi --lib --crate-type staticlib
	@echo "built $(OUTDIR)/libnoroi.so and $(OUTDIR)/libnoroi.a"

cdemo: capi
	$(CC) examples/demo.c -Iinclude -L$(OUTDIR) -lnoroi -lpthread -ldl -lm -o noroidemo_c
	@echo "built ./noroidemo_c"

clean:
	$(CARGO) clean
	rm -f noroidemo_c
