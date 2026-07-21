-- SQLite-specific dialect sample for SQL chunker stress testing
-- Covers: AUTOINCREMENT, WITHOUT ROWID, VIRTUAL TABLE fts5, PRAGMA statements,
-- ATTACH database, IF NOT EXISTS, triggers, no schema qualification

-- ============================================================
-- PRAGMA configuration (SQLite-specific, no semicolons optional)
-- ============================================================

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -8000;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;

-- ============================================================
-- CREATE TABLE IF NOT EXISTS with AUTOINCREMENT
-- ============================================================

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    bio TEXT DEFAULT '',
    avatar_url TEXT,
    is_admin INTEGER NOT NULL DEFAULT 0 CHECK (is_admin IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================
-- WITHOUT ROWID table (clustered index on PRIMARY KEY)
-- ============================================================

CREATE TABLE IF NOT EXISTS user_settings (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    setting_key TEXT NOT NULL,
    setting_value TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (user_id, setting_key)
) WITHOUT ROWID;

-- ============================================================
-- Regular tables with various constraint styles
-- ============================================================

CREATE TABLE IF NOT EXISTS posts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    is_published INTEGER NOT NULL DEFAULT 0,
    view_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE IF NOT EXISTS post_tags (
    post_id INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (post_id, tag_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_posts_user ON posts(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_posts_slug ON posts(slug);
CREATE INDEX IF NOT EXISTS idx_posts_published ON posts(is_published) WHERE is_published = 1;

-- ============================================================
-- VIRTUAL TABLE with FTS5 (full-text search)
-- ============================================================

CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(
    title,
    body,
    content='posts',
    content_rowid='id',
    tokenize='porter unicode61 remove_diacritics 2'
);

-- ============================================================
-- Triggers to keep FTS index synchronized
-- ============================================================

CREATE TRIGGER IF NOT EXISTS posts_ai AFTER INSERT ON posts BEGIN
    INSERT INTO posts_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
END;

CREATE TRIGGER IF NOT EXISTS posts_ad AFTER DELETE ON posts BEGIN
    INSERT INTO posts_fts(posts_fts, rowid, title, body)
        VALUES ('delete', old.id, old.title, old.body);
END;

CREATE TRIGGER IF NOT EXISTS posts_au AFTER UPDATE ON posts BEGIN
    INSERT INTO posts_fts(posts_fts, rowid, title, body)
        VALUES ('delete', old.id, old.title, old.body);
    INSERT INTO posts_fts(rowid, title, body)
        VALUES (new.id, new.title, new.body);
END;

-- ============================================================
-- Trigger for updated_at auto-update
-- ============================================================

CREATE TRIGGER IF NOT EXISTS trg_users_updated_at
    AFTER UPDATE ON users
    FOR EACH ROW
    WHEN old.updated_at = new.updated_at
BEGIN
    UPDATE users SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_posts_updated_at
    AFTER UPDATE ON posts
    FOR EACH ROW
    WHEN old.updated_at = new.updated_at
BEGIN
    UPDATE posts SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;

-- ============================================================
-- ATTACH database (cross-database queries)
-- ============================================================

ATTACH DATABASE ':memory:' AS analytics;

CREATE TABLE analytics.page_views (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    post_id INTEGER,
    referrer TEXT,
    user_agent TEXT,
    viewed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX analytics.idx_views_post ON analytics.page_views(post_id, viewed_at);

-- ============================================================
-- DML: INSERTs, SELECTs, full-text queries
-- ============================================================

INSERT INTO users (username, email, password_hash, display_name, is_admin)
VALUES ('admin', 'admin@blog.local', 'argon2id$v=19$m=65536,t=3,p=4$hash', 'Admin', 1);

INSERT INTO users (username, email, password_hash, display_name)
VALUES ('alice', 'alice@example.com', 'argon2id$v=19$m=65536,t=3,p=4$hash2', 'Alice');

INSERT INTO posts (user_id, title, body, slug, is_published)
VALUES (1, 'Getting Started with SQLite FTS5', 'Full-text search in SQLite is powerful...', 'sqlite-fts5-guide', 1);

INSERT INTO tags (name) VALUES ('sqlite'), ('tutorial'), ('search');

INSERT INTO post_tags (post_id, tag_id) VALUES (1, 1), (1, 2), (1, 3);

-- Full-text search query using FTS5
SELECT p.id, p.title, snippet(posts_fts, 1, '<b>', '</b>', '...', 32) AS excerpt,
       highlight(posts_fts, 0, '<mark>', '</mark>') AS highlighted_title
FROM posts_fts
JOIN posts p ON p.id = posts_fts.rowid
WHERE posts_fts MATCH 'sqlite AND search'
ORDER BY rank;

-- Cross-database join with the attached analytics DB
SELECT p.title, COUNT(pv.id) AS views, u.display_name AS author
FROM posts p
JOIN users u ON p.user_id = u.id
LEFT JOIN analytics.page_views pv ON pv.post_id = p.id
WHERE p.is_published = 1
GROUP BY p.id
ORDER BY views DESC
LIMIT 10;

-- ============================================================
-- FTS5 maintenance commands
-- ============================================================

INSERT INTO posts_fts(posts_fts) VALUES ('optimize');

-- ============================================================
-- PRAGMA integrity check (common maintenance)
-- ============================================================

PRAGMA integrity_check;
PRAGMA optimize;
