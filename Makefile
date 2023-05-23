build:
	if [ "$$(id -u)" -ne 0 -o -z "$$SUDO_USER" ]; then cargo build --release; else sudo -u "$$SUDO_USER" cargo build --release; fi;

check:
	if [ "$$(id -u)" -ne 0 -o -z "$$SUDO_USER" ]; then cargo test --release; else sudo -u "$$SUDO_USER" cargo test --release; fi;

clean:
	cargo clean

distclean: clean

install: build
	install -m 755 target/release/fasthtcacheclean /usr/local/bin/fasthtcacheclean
	install -d /usr/local/share/man/man8
	install -m 644 target/release/fasthtcacheclean.8 /usr/local/share/man/man8/fasthtcacheclean.8
	install -d /usr/local/lib/systemd/system/
	install -m 644 fasthtcacheclean.service /usr/local/lib/systemd/system/fasthtcacheclean.service
	install -m 644 fasthtcacheclean.timer /usr/local/lib/systemd/system/fasthtcacheclean.timer
	systemctl daemon-reload || true
	systemctl enable fasthtcacheclean.timer || true


uninstall:
	systemctl stop fasthtcacheclean.timer fasthtcacheclean.service || true
	systemctl disable fasthtcacheclean.timer || true
	rm /usr/local/lib/systemd/system/fasthtcacheclean.timer
	rm /usr/local/lib/systemd/system/fasthtcacheclean.service
	systemctl daemon-reload || true
	rm /usr/local/share/man/man8/fasthtcacheclean.8 || true
	rm /usr/local/bin/fasthtcacheclean || true

.PHONY: build clean distclean install uninstall check
