#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Build resources/sample.sqlite, a synthetic database for trying dbviewer.

All data is generated from a fixed seed; nothing comes from a real database.
Run `python3 resources/make_sample_db.py` to recreate the file.
"""

import json
import random
import sqlite3
from datetime import date, datetime, timedelta
from pathlib import Path

OUT = Path(__file__).with_name("sample.sqlite")

FIRST = ["Ada", "Grace", "Linus", "Margaret", "Ken", "Barbara", "Dennis", "Frances",
         "Alan", "Radia", "Edsger", "Hedy", "Niklaus", "Sophie", "Tim", "Zoë"]
LAST = ["Lovelace", "Hopper", "Torvalds", "Hamilton", "Thompson", "Liskov", "Ritchie",
        "Allen", "Turing", "Perlman", "Dijkstra", "Lamarr", "Wirth", "Wilson", "Łukasiewicz"]
CITIES = ["Kraków", "Berlin", "Lisboa", "São Paulo", "東京", "Reykjavík", "Zürich", "Oslo"]
STATUSES = ["pending", "paid", "shipped", "delivered", "cancelled", "refunded"]
PRODUCTS = [("Keyboard", "hardware"), ("Mechanical switch set", "hardware"),
            ("USB-C cable", "accessories"), ("Monitor arm", "accessories"),
            ("Editor licence", "software"), ("Theme pack", "software"),
            ("Terminal stickers", "merch"), ("Mug — modal edition", "merch")]


def build(db: sqlite3.Connection, rng: random.Random) -> None:
    db.executescript(
        '''
        CREATE TABLE customers (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT UNIQUE,
            city TEXT,
            signed_up TEXT,
            vip INTEGER NOT NULL DEFAULT 0,
            notes TEXT
        );
        CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            category TEXT NOT NULL,
            price NUMERIC NOT NULL,
            attributes TEXT
        );
        CREATE TABLE orders (
            id INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL REFERENCES customers(id),
            product_id INTEGER NOT NULL REFERENCES products(id),
            quantity INTEGER NOT NULL,
            unit_price NUMERIC NOT NULL,
            status TEXT NOT NULL,
            ordered_at TEXT NOT NULL,
            shipped_at TEXT,
            discount REAL,
            channel TEXT,
            coupon TEXT,
            gift INTEGER,
            metadata TEXT
        );
        CREATE INDEX orders_customer ON orders(customer_id);
        CREATE INDEX orders_status ON orders(status);

        -- No primary key or rowid alias: ordering here is only by column values.
        CREATE TABLE events (
            at TEXT,
            level TEXT,
            message TEXT
        );

        -- One row per interesting value, to try previews, Show full value and JSON.
        CREATE TABLE edge_cases (
            id INTEGER PRIMARY KEY,
            label TEXT NOT NULL,
            value
        );

        CREATE TABLE "odd names" (
            "id" INTEGER PRIMARY KEY,
            "select" TEXT,
            "column with spaces" TEXT,
            "Mixed ""Quotes""" TEXT,
            "zażółć" TEXT
        );

        CREATE VIEW order_totals AS
            SELECT c.id AS customer_id, c.name, count(o.id) AS orders,
                   round(sum(o.quantity * o.unit_price * (1 - coalesce(o.discount, 0))), 2) AS total
            FROM customers c LEFT JOIN orders o ON o.customer_id = c.id
            GROUP BY c.id;
        '''
    )

    start = date(2024, 1, 1)
    customers = []
    for i in range(1, 201):
        first, last = rng.choice(FIRST), rng.choice(LAST)
        email = None if i % 17 == 0 else f"{first}.{last}.{i}@example.test".lower()
        notes = rng.choice([None, "", "Prefers email.", "Line one\nLine two\n\tIndented",
                            "Asked about the 🦀 edition."])
        customers.append((i, f"{first} {last}", email, rng.choice(CITIES + [None]),
                          (start + timedelta(days=rng.randrange(600))).isoformat(),
                          int(rng.random() < 0.1), notes))
    db.executemany("INSERT INTO customers VALUES (?,?,?,?,?,?,?)", customers)

    for i, (name, category) in enumerate(PRODUCTS, 1):
        attributes = json.dumps({"sku": f"RY-{i:04d}", "tags": [category, "sample"],
                                 "dimensions": {"w": rng.randint(5, 60), "h": rng.randint(1, 30)},
                                 "in_stock": rng.random() < 0.8}, ensure_ascii=False)
        db.execute("INSERT INTO products VALUES (?,?,?,?,?)",
                   (i, name, category, f"{rng.uniform(3, 250):.2f}", attributes))

    # 2,500 orders: more than one 1,000-row page and the SQL result limit.
    orders = []
    epoch = datetime(2024, 1, 1, 8)
    for i in range(1, 2501):
        product = rng.randint(1, len(PRODUCTS))
        status = rng.choice(STATUSES)
        ordered = epoch + timedelta(minutes=rng.randrange(60 * 24 * 600))
        shipped = (ordered + timedelta(days=rng.randint(1, 9))).isoformat(" ", "seconds") \
            if status in ("shipped", "delivered") else None
        metadata = None if i % 5 else json.dumps({"ip": f"192.0.2.{i % 250}",
                                                  "agent": "runyte-sample/1.0",
                                                  "items": [{"line": n} for n in range(i % 4)]})
        orders.append((i, rng.randint(1, 200), product, rng.randint(1, 5),
                       f"{rng.uniform(3, 250):.2f}", status, ordered.isoformat(" ", "seconds"),
                       shipped, rng.choice([None, 0.05, 0.1, 0.25]),
                       rng.choice(["web", "store", "phone"]), rng.choice([None, "", "SPRING24"]),
                       rng.choice([0, 1, None]), metadata))
    db.executemany("INSERT INTO orders VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)", orders)

    levels = ["DEBUG", "INFO", "WARN", "ERROR"]
    db.executemany("INSERT INTO events VALUES (?,?,?)", [
        ((epoch + timedelta(seconds=37 * n)).isoformat(" ", "seconds"),
         rng.choice(levels), f"event {n % 40}")  # repeated values: unstable ordering
        for n in range(300)
    ])

    nested = {"level": 0}
    cursor = nested
    for depth in range(1, 80):  # deeper than the 64-level preview tree
        cursor["child"] = {"level": depth}
        cursor = cursor["child"]
    edge_cases = [
        ("NULL", None),
        ("empty string", ""),
        ("integer", 42),
        ("negative real", -3.14159),
        ("huge integer", 9223372036854775807),
        ("numeric text", "00123.4500"),
        ("unicode", "Zażółć gęślą jaźń — 日本語 — emoji 🦀🚀 — RTL שלום"),
        ("control characters", "tab\there\nnewline\rcarriage\x00nul\x1bescape\x7fdel"),
        ("small blob", bytes(range(32))),
        ("invalid UTF-8 blob", b"valid then \xff\xfe invalid \xc3"),
        ("small JSON object", json.dumps({"name": "Runyte", "modes": ["normal", "insert", "select"],
                                          "stable": True, "version": None})),
        ("JSON array", json.dumps([1, 2.50, "three", {"four": [4]}, None, False])),
        ("JSON duplicate keys and exact numbers", '{"a": 1, "a": 2, "big": 1.000000000000000000001}'),
        ("deep JSON (80 levels)", json.dumps(nested)),
        ("malformed JSON", '{"unterminated": [1, 2, 3'),
        ("wide JSON (12,000 lines formatted)", json.dumps({f"key_{n:05d}": n for n in range(12000)})),
        ("long text (~150 KiB)", "\n".join(f"{n:05d} The quick brown fox jumps over the lazy dog."
                                           for n in range(3200))),
        ("single long line (100 KiB)", "x" * 100 * 1024),
    ]
    db.executemany("INSERT INTO edge_cases (label, value) VALUES (?,?)", edge_cases)

    db.executemany('INSERT INTO "odd names" VALUES (?,?,?,?,?)', [
        (1, "reserved word", "spaces", 'embedded "quotes"', "Polish letters"),
        (2, None, "", "%_\\ literal wildcards", "ąęłńóśźż"),
    ])


def main() -> None:
    OUT.unlink(missing_ok=True)
    db = sqlite3.connect(OUT)
    with db:
        build(db, random.Random(20260924))
    db.execute("VACUUM")
    db.close()
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB)")


if __name__ == "__main__":
    main()
