# Apache Disk Cache Clean Tool

Ein rudimentärer Ersatz für `apache-htcacheclean`, der deutlich schneller den Cache bereinigt.

## Installieren

Zum Bau wird wird `cargo` benötigt:

```sh
	sudo apt install cargo
```

Dann kann das Programm mit

```sh
	cargo build --release
```
gebaut werden.

Das Ergebnis liegt dann in `target/release/papoo-htcacheclean`.

Zum Installieren:

```
cp target/release/papoo-htcacheclean /usr/local/bin/
cp papoo-htcacheclean.{service,timer} /etc/systemd/system/
systemctl daemon-reload
systemctl disable apache-htcacheclean
systemctl enable papoo-htcacheclean.timer
systemctl start papoo-htcacheclean.timer
```

## Funktionsweise

Die Funktionsweise ist ähnlich wie bei `apache-htcacheclean` mit einigen Optimierungen:

1. Zunächst wird geprüft, ob das Limit bereits überschritten oder fast erreicht ist. Ab 90 % Auslastung werden die ersten Dateien gelöscht.
2. Dann werden alte temporäre Dateien im Hauptverzeichnis des Caches gelöscht (länger als 15 Minuten nicht modizifiert).
3. Dann wird der Verzeichnisbaum durchsucht (standardmäßig mit CPUs/2 Threads gleichzeitig).
   Dabei werden alte leere Verzeichnisse und verwaiste `.data`-Dateien direkt gelöscht.
   Die Einträge werden dabei nach Expiry-Datum, Access-Datum und Modification-Datum in eine Priority-Queue einsortiert.
   Um den RAM-Bedarf gering zu halten, werden nur die 1.000.000 ältesten Einträge berücksichtigt.
4. Die gefundenen Cache-Einträge werden dann beginnend mit dem ältesten gelöscht, bis nur noch 99.0 bis 99.5 % des Limits verwendet werden.
