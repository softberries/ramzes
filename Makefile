PG_PORT := 55432
PG_NAME := ramzes-test-pg
PG_URL := postgres://postgres:ramzes@127.0.0.1:$(PG_PORT)/postgres

.PHONY: test test-pg pg-up pg-down lint ui build run

# SQLite only. Postgres tests print a skip notice.
test:
	cargo nextest run --workspace

# Starts a throwaway Postgres, runs every test against both databases, removes it.
test-pg: pg-up
	RAMZES_TEST_POSTGRES_URL=$(PG_URL) cargo nextest run --workspace; s=$$?; $(MAKE) pg-down; exit $$s

pg-up:
	-docker rm -f $(PG_NAME) >/dev/null 2>&1
	docker run -d --name $(PG_NAME) -e POSTGRES_PASSWORD=ramzes -p 127.0.0.1:$(PG_PORT):5432 postgres:18 >/dev/null
	@until docker exec $(PG_NAME) pg_isready -U postgres >/dev/null 2>&1; do sleep 0.5; done; sleep 1

pg-down:
	-docker rm -f $(PG_NAME) >/dev/null 2>&1

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

ui:
	npm --prefix ui ci
	npm --prefix ui run build

build: ui
	cargo build --release -p ramzes-server

run:
	cargo run -p ramzes-server
