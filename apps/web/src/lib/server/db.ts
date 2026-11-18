import Database from 'better-sqlite3';
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { DATABASE_PATH } from '$app/env/private';

/**
 * SQLite database (one file). Schema is applied idempotently on open; bump
 * SCHEMA_VERSION and add a step to MIGRATIONS for changes.
 */
export type DB = Database.Database;

const MIGRATIONS: string[] = [
	`CREATE TABLE users (
		id TEXT PRIMARY KEY,
		email TEXT UNIQUE,
		name TEXT NOT NULL DEFAULT '',
		password_hash TEXT,
		google_id TEXT UNIQUE,
		created_at INTEGER NOT NULL
	);
	CREATE TABLE sessions (
		id TEXT PRIMARY KEY,
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		expires_at INTEGER NOT NULL
	);
	CREATE INDEX sessions_user ON sessions(user_id);
	CREATE TABLE attempts (
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		at INTEGER NOT NULL,
		data TEXT NOT NULL,
		PRIMARY KEY (user_id, at)
	);
	CREATE TABLE drill_results (
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		at INTEGER NOT NULL,
		data TEXT NOT NULL,
		PRIMARY KEY (user_id, at)
	);
	CREATE TABLE thumbs (
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		at INTEGER NOT NULL,
		data_url TEXT NOT NULL,
		PRIMARY KEY (user_id, at)
	);`,
	`CREATE TABLE tip_votes (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		rule_id TEXT NOT NULL,
		vote INTEGER NOT NULL CHECK (vote IN (-1, 1)),
		category TEXT NOT NULL DEFAULT '',
		score REAL,
		attempt_at INTEGER,
		user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
		created_at INTEGER NOT NULL
	);
	CREATE INDEX tip_votes_rule ON tip_votes(rule_id);
	CREATE UNIQUE INDEX tip_votes_once ON tip_votes(user_id, attempt_at, rule_id) WHERE user_id IS NOT NULL;`
];

export function openDb(path: string): DB {
	if (path !== ':memory:') mkdirSync(dirname(path), { recursive: true });
	const db = new Database(path);
	db.pragma('journal_mode = WAL');
	db.pragma('foreign_keys = ON');
	const version = db.pragma('user_version', { simple: true }) as number;
	for (let v = version; v < MIGRATIONS.length; v++) {
		db.transaction(() => {
			db.exec(MIGRATIONS[v]);
			db.pragma(`user_version = ${v + 1}`);
		})();
	}
	return db;
}

let db: DB | undefined;

/** The app database. `DATABASE_PATH` (default: <repo>/data/pencilpilot.db, git-ignored). */
export function getDb(): DB {
	db ??= openDb(DATABASE_PATH || resolve(process.cwd(), '../../data/pencilpilot.db'));
	return db;
}
