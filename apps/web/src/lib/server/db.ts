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
	CREATE UNIQUE INDEX tip_votes_once ON tip_votes(user_id, attempt_at, rule_id) WHERE user_id IS NOT NULL;`,
	// community critique (lib/server/community.ts)
	`CREATE TABLE shared_drawings (
		id TEXT PRIMARY KEY,
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		attempt_at INTEGER,
		reference_id TEXT NOT NULL,
		reference_json TEXT NOT NULL,
		category TEXT NOT NULL,
		png BLOB NOT NULL,
		score REAL,
		rating REAL NOT NULL DEFAULT 1000,
		comparisons INTEGER NOT NULL DEFAULT 0,
		wins INTEGER NOT NULL DEFAULT 0,
		hidden INTEGER NOT NULL DEFAULT 0,
		created_at INTEGER NOT NULL
	);
	CREATE INDEX shared_ref ON shared_drawings(reference_id, hidden);
	CREATE INDEX shared_user ON shared_drawings(user_id, created_at);
	CREATE UNIQUE INDEX shared_attempt ON shared_drawings(user_id, attempt_at) WHERE attempt_at IS NOT NULL;
	CREATE TABLE closer_votes (
		voter_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		a_id TEXT NOT NULL REFERENCES shared_drawings(id) ON DELETE CASCADE,
		b_id TEXT NOT NULL REFERENCES shared_drawings(id) ON DELETE CASCADE,
		winner_id TEXT NOT NULL,
		created_at INTEGER NOT NULL,
		PRIMARY KEY (voter_id, a_id, b_id)
	);
	CREATE TABLE rubric_ratings (
		rater_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		drawing_id TEXT NOT NULL REFERENCES shared_drawings(id) ON DELETE CASCADE,
		proportions INTEGER NOT NULL CHECK (proportions BETWEEN 1 AND 5),
		lines INTEGER NOT NULL CHECK (lines BETWEEN 1 AND 5),
		shading INTEGER NOT NULL CHECK (shading BETWEEN 1 AND 5),
		overall INTEGER NOT NULL CHECK (overall BETWEEN 1 AND 5),
		created_at INTEGER NOT NULL,
		PRIMARY KEY (rater_id, drawing_id)
	);
	CREATE INDEX rubric_drawing ON rubric_ratings(drawing_id);
	CREATE TABLE drawing_reports (
		reporter_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		drawing_id TEXT NOT NULL REFERENCES shared_drawings(id) ON DELETE CASCADE,
		reason TEXT NOT NULL,
		created_at INTEGER NOT NULL,
		PRIMARY KEY (reporter_id, drawing_id)
	);`,
	// daily challenge (lib/server/daily.ts): first scored try per person per day
	`CREATE TABLE daily_scores (
		day TEXT NOT NULL,
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		ref_id TEXT NOT NULL,
		score REAL NOT NULL CHECK (score BETWEEN 1 AND 10),
		attempt_at INTEGER,
		created_at INTEGER NOT NULL,
		PRIMARY KEY (day, user_id)
	);
	CREATE INDEX daily_user ON daily_scores(user_id, day);`,
	// the desktop app linked to an account, for drawing together (lib/server/applink.ts):
	// one-time codes on the way, then the app's token. Only hashes are stored.
	`CREATE TABLE app_link_codes (
		id TEXT PRIMARY KEY,
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		challenge TEXT NOT NULL,
		redirect_uri TEXT NOT NULL,
		label TEXT NOT NULL,
		expires_at INTEGER NOT NULL
	);
	CREATE TABLE app_links (
		id TEXT PRIMARY KEY,
		token_hash TEXT NOT NULL UNIQUE,
		user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
		label TEXT NOT NULL,
		created_at INTEGER NOT NULL,
		last_used_at INTEGER
	);
	CREATE INDEX app_links_user ON app_links(user_id);`
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
