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

Anders als `apache-htcacheclean`, welches zunächst alle Dateien des Caches auflistet, nach Größe sortiert und dann erst beginnt zu löschen,
arbeitet dieses Script direkter und schneller in mehreren Runden, löscht dafür u.U. mehr als nötig.

- Es wird mit CPUs/2 Threads gleichzeitig gearbeitet.
- Es wird bestimmt, wie viel Speicher und wie viele Inodes noch auf der Partition frei sind.
- Der freie Speicherplatz wird mit dem festgelegten Limit verglichen
  - Wenn nicht mehr genug Speicher verfügbar ist (99,5 % des Limits oder mehr erreicht), wird die folgende Liste abgearbeitet, bis weniger als 100% verbraucht werden.
    1. Expiry länger als eine Stunde her
    2. Expiry länger 30 Minuten her
    3. Expiry länger 10 Minuten her
    4. Expiry länger als 1 Minute her
    5. Letzter Zugriff länger als 30 Minuten her
    6. Letzter Zugriff länger als 10 Minuten her
    7. Letzter Zugriff länger als 2 Minuten her
    8. Bearbeitung länger als 10 Minuten her
    9. Bearbeitung länger als 2 Minuten her
  - Wenn 95 % erreicht sind, wird nur alles mit Expiry > 3 Stunden gelöscht
  - Wenn 90 % erreicht sind, wird nur alles mit Expiry > 6 Stunden gelöscht
  - Ansonsten wird nur alles mit Expiry > 24 Stunden gelöscht

Die Grenzen werden noch anhand der empirischen Ergebnisse auf dem 5f-Server angepasst.
