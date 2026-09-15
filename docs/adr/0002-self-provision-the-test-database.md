# Self-provision an embedded PostgreSQL server for tests

Tests first use a reachable database from `DB_URL`, then the `.env` file, and
then the CI fallback URL. An external server gets a dedicated per-process test
database. If no configured server is reachable, tests start one embedded
PostgreSQL 17 server on an ephemeral port. This keeps local tests independent
of Docker while preserving the existing CI service path.

The database selection is made once per test process. A real authenticated
connection is required before an external server is selected. Test cleanup
must not alter the configured database. The process-owned database is dropped
when the process exits, and the embedded postmaster receives a termination
signal.

TLS settings are part of the same connection policy. Plain, `disable`, and
`prefer` URLs use an unencrypted connection. `require` uses Rustls with the
operating system trust store and certificate verification enabled. Unsupported
TLS modes fail explicitly. A required-TLS connection failure never falls back
to the embedded server. Diesel migrations do not provide TLS support in this
stack, so a required-TLS URL fails clearly during migration rather than being
downgraded.
