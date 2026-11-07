-- OpenDecks Session library schema, version 1.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE track (
    id               INTEGER PRIMARY KEY,
    path             TEXT    NOT NULL UNIQUE,
    content_hash     TEXT    NOT NULL,
    file_size        INTEGER NOT NULL,
    format           TEXT,
    duration         REAL    NOT NULL DEFAULT 0,
    sample_rate      INTEGER NOT NULL DEFAULT 0,
    channels         INTEGER NOT NULL DEFAULT 2,
    bit_rate         INTEGER,
    bits_per_sample  INTEGER,
    title            TEXT    NOT NULL DEFAULT '',
    artist           TEXT    NOT NULL DEFAULT '',
    album            TEXT    NOT NULL DEFAULT '',
    genre            TEXT    NOT NULL DEFAULT '',
    label            TEXT    NOT NULL DEFAULT '',
    remixer          TEXT    NOT NULL DEFAULT '',
    composer         TEXT    NOT NULL DEFAULT '',
    comment          TEXT    NOT NULL DEFAULT '',
    year             INTEGER,
    track_number     INTEGER,
    rating           INTEGER NOT NULL DEFAULT 0,
    color            TEXT,
    date_added       TEXT    NOT NULL,
    play_count       INTEGER NOT NULL DEFAULT 0,
    last_played      TEXT,
    missing          INTEGER NOT NULL DEFAULT 0,
    -- Denormalised from `analysis` for fast sorting and filtering.
    bpm              REAL,
    musical_key      TEXT,
    analysis_version INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX track_hash ON track (content_hash);

CREATE VIRTUAL TABLE track_fts USING fts5 (
    title, artist, album, genre, label, remixer, comment,
    content = 'track', content_rowid = 'id', tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER track_ai AFTER INSERT ON track BEGIN
    INSERT INTO track_fts (rowid, title, artist, album, genre, label, remixer, comment)
    VALUES (new.id, new.title, new.artist, new.album, new.genre, new.label, new.remixer, new.comment);
END;
CREATE TRIGGER track_ad AFTER DELETE ON track BEGIN
    INSERT INTO track_fts (track_fts, rowid, title, artist, album, genre, label, remixer, comment)
    VALUES ('delete', old.id, old.title, old.artist, old.album, old.genre, old.label, old.remixer, old.comment);
END;
CREATE TRIGGER track_au AFTER UPDATE OF title, artist, album, genre, label, remixer, comment ON track BEGIN
    INSERT INTO track_fts (track_fts, rowid, title, artist, album, genre, label, remixer, comment)
    VALUES ('delete', old.id, old.title, old.artist, old.album, old.genre, old.label, old.remixer, old.comment);
    INSERT INTO track_fts (rowid, title, artist, album, genre, label, remixer, comment)
    VALUES (new.id, new.title, new.artist, new.album, new.genre, new.label, new.remixer, new.comment);
END;

CREATE TABLE artwork (
    track_id   INTEGER PRIMARY KEY REFERENCES track (id) ON DELETE CASCADE,
    media_type TEXT NOT NULL,
    data       BLOB NOT NULL
);

CREATE TABLE analysis (
    track_id      INTEGER PRIMARY KEY REFERENCES track (id) ON DELETE CASCADE,
    version       INTEGER NOT NULL,
    grid          BLOB,
    grid_source   TEXT,
    grid_locked   INTEGER NOT NULL DEFAULT 0,
    key_source    TEXT,
    waveform      BLOB,
    waveform_rate REAL,
    phrases       TEXT,
    phrase_mood   TEXT,
    loudness      REAL,
    peak          REAL,
    energy        BLOB
);

CREATE TABLE cue (
    id       INTEGER PRIMARY KEY,
    track_id INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
    kind     TEXT    NOT NULL,            -- 'hot' | 'memory'
    slot     INTEGER NOT NULL DEFAULT 0,
    start    REAL    NOT NULL,
    loop_end REAL,
    label    TEXT    NOT NULL DEFAULT '',
    color    TEXT,
    source   TEXT    NOT NULL,
    active   INTEGER NOT NULL DEFAULT 0   -- active loop (schema 3)
);
CREATE INDEX cue_track ON cue (track_id);

-- Folders, playlists, smart playlists and sets form one tree.
CREATE TABLE node (
    id         INTEGER PRIMARY KEY,
    parent_id  INTEGER REFERENCES node (id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL,          -- 'folder' | 'playlist' | 'smart' | 'set'
    name       TEXT    NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    smart_rule TEXT,
    set_meta   TEXT,
    created    TEXT    NOT NULL
);
CREATE INDEX node_parent ON node (parent_id);

CREATE TABLE entry (
    node_id  INTEGER NOT NULL REFERENCES node (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    track_id INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
    note     TEXT    NOT NULL DEFAULT '',
    backups  TEXT    NOT NULL DEFAULT '[]',
    PRIMARY KEY (node_id, position)
);
CREATE INDEX entry_track ON entry (track_id);

CREATE TABLE device (
    id          INTEGER PRIMARY KEY,
    volume_uuid TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    formats     TEXT NOT NULL,
    synced_seq  INTEGER NOT NULL DEFAULT 0,
    last_sync   TEXT,
    last_error  TEXT
);

CREATE TABLE device_node (
    device_id INTEGER NOT NULL REFERENCES device (id) ON DELETE CASCADE,
    node_id   INTEGER NOT NULL REFERENCES node (id) ON DELETE CASCADE,
    PRIMARY KEY (device_id, node_id)
);

-- Monotonic change log used to find out-of-date devices.
CREATE TABLE change (
    seq      INTEGER PRIMARY KEY AUTOINCREMENT,
    kind     TEXT NOT NULL,
    track_id INTEGER,
    node_id  INTEGER,
    at       TEXT NOT NULL
);

-- Undo journal of user edits. `before`/`after` hold JSON snapshots.
CREATE TABLE journal (
    id     INTEGER PRIMARY KEY,
    label  TEXT    NOT NULL,
    before TEXT    NOT NULL,
    after  TEXT    NOT NULL,
    at     TEXT    NOT NULL,
    undone INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE recording (
    id        INTEGER PRIMARY KEY,
    path      TEXT NOT NULL,
    started   TEXT NOT NULL,
    duration  REAL NOT NULL DEFAULT 0,
    tracklist TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE history (
    id        INTEGER PRIMARY KEY,
    session   TEXT    NOT NULL,
    played_at TEXT    NOT NULL,
    track_id  INTEGER REFERENCES track (id) ON DELETE SET NULL,
    source    TEXT    NOT NULL           -- 'opendecks' | 'device'
);
