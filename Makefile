# Author: Lukas Bower

.PHONY: esp run-uefi clean-esp

esp:
	./scripts/esp-build.sh

run-uefi: esp
	./scripts/qemu-uefi-aarch64.sh

clean-esp:
	rm -f out/cohesix/esp.img out/cohesix/edk2_vars.fd
