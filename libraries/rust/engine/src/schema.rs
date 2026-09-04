//! The Engine Library schema, version 2.21.2.
//!
//! This is Denon's schema, not ours. A player checks what it finds: a missing
//! column or an absent trigger and the database is not read, so the statements
//! below are the format rather than a design.
//!
//! Transcribed from [libdjinterop](https://github.com/xsco/libdjinterop),
//! `src/djinterop/engine/schema/schema_2_21_2.cpp`, which is the open record of
//! each Engine schema version. Version 2.x is what Engine DJ 2 and 3 read; the
//! 1.x layout with its separate `m.db` and `p.db` belongs to Engine Prime and
//! the first SC5000 firmware.
//!
//! Several of these triggers do work on insert: the origin columns and the
//! playlist link fields are filled in by the database itself, which is why the
//! writer leaves them alone.

use rusqlite::Connection;

/// Major, minor and patch, as written into the `Information` row.
pub const VERSION: (i32, i32, i32) = (2, 21, 2);

/// Every statement that makes an empty Engine database, in order.
pub const STATEMENTS: [&str; 54] = [
    r#"CREATE TABLE Information ( id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT, schemaVersionMajor INTEGER, schemaVersionMinor INTEGER, schemaVersionPatch INTEGER, currentPlayedIndiciator INTEGER, lastRekordBoxLibraryImportReadCounter INTEGER);"#,
    r#"CREATE TABLE AlbumArt ( id INTEGER PRIMARY KEY AUTOINCREMENT, hash TEXT, albumArt BLOB );"#,
    r#"CREATE TABLE Pack ( id INTEGER PRIMARY KEY AUTOINCREMENT, packId TEXT, changeLogDatabaseUuid TEXT, changeLogId INTEGER, lastPackTime DATETIME );"#,
    r#"CREATE TABLE Track ( id INTEGER PRIMARY KEY AUTOINCREMENT, playOrder INTEGER, length INTEGER, bpm INTEGER, year INTEGER, path TEXT, filename TEXT, bitrate INTEGER, bpmAnalyzed REAL, albumArtId INTEGER, fileBytes INTEGER, title TEXT, artist TEXT, album TEXT, genre TEXT, comment TEXT, label TEXT, composer TEXT, remixer TEXT, key INTEGER, rating INTEGER, albumArt TEXT, timeLastPlayed DATETIME, isPlayed BOOLEAN, fileType TEXT, isAnalyzed BOOLEAN, dateCreated DATETIME, dateAdded DATETIME, isAvailable BOOLEAN, isMetadataOfPackedTrackChanged BOOLEAN, isPerfomanceDataOfPackedTrackChanged BOOLEAN, playedIndicator INTEGER, isMetadataImported BOOLEAN, pdbImportKey INTEGER, streamingSource TEXT, uri TEXT, isBeatGridLocked BOOLEAN, originDatabaseUuid TEXT, originTrackId INTEGER, trackData BLOB, overviewWaveFormData BLOB, beatData BLOB, quickCues BLOB, loops BLOB, thirdPartySourceId INTEGER, streamingFlags INTEGER, explicitLyrics BOOLEAN, activeOnLoadLoops INTEGER, lastEditTime DATETIME, CONSTRAINT C_originDatabaseUuid_originTrackId UNIQUE (originDatabaseUuid, originTrackId), CONSTRAINT C_path UNIQUE (path), FOREIGN KEY (albumArtId) REFERENCES AlbumArt (id) ON DELETE RESTRICT );"#,
    r#"CREATE TABLE Playlist ( id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, parentListId INTEGER, isPersisted BOOLEAN, nextListId INTEGER, lastEditTime DATETIME, isExplicitlyExported BOOLEAN, CONSTRAINT C_NAME_UNIQUE_FOR_PARENT UNIQUE (title, parentListId), CONSTRAINT C_NEXT_LIST_ID_UNIQUE_FOR_PARENT UNIQUE (parentListId, nextListId) );"#,
    r#"CREATE TABLE PlaylistEntity ( id INTEGER PRIMARY KEY AUTOINCREMENT, listId INTEGER, trackId INTEGER, databaseUuid TEXT, nextEntityId INTEGER, membershipReference INTEGER, CONSTRAINT C_NAME_UNIQUE_FOR_LIST UNIQUE (listId, databaseUuid, trackId), FOREIGN KEY (listId) REFERENCES Playlist (id) ON DELETE CASCADE );"#,
    r#"CREATE TABLE PreparelistEntity ( id INTEGER PRIMARY KEY AUTOINCREMENT, trackId INTEGER, trackNumber INTEGER, FOREIGN KEY (trackId) REFERENCES Track (id) ON DELETE CASCADE );"#,
    r#"CREATE TABLE Smartlist ( listUuid TEXT NOT NULL PRIMARY KEY, title TEXT, parentPlaylistPath TEXT, nextPlaylistPath TEXT, nextListUuid TEXT, rules TEXT, lastEditTime DATETIME, CONSTRAINT C_NAME_UNIQUE_FOR_PARENT UNIQUE (title, parentPlaylistPath), CONSTRAINT C_NEXT_LIST_UNIQUE_FOR_PARENT UNIQUE (parentPlaylistPath, nextPlaylistPath, nextListUuid) );"#,
    r#"CREATE INDEX index_AlbumArt_hash ON AlbumArt (hash);"#,
    r#"CREATE INDEX index_Track_filename ON Track (filename);"#,
    r#"CREATE INDEX index_Track_albumArtId ON Track (albumArtId);"#,
    r#"CREATE INDEX index_Track_uri ON Track (uri);"#,
    r#"CREATE INDEX index_Track_title ON Track(title);"#,
    r#"CREATE INDEX index_Track_length ON Track(length);"#,
    r#"CREATE INDEX index_Track_rating ON Track(rating);"#,
    r#"CREATE INDEX index_Track_year ON Track(year);"#,
    r#"CREATE INDEX index_Track_dateAdded ON Track(dateAdded);"#,
    r#"CREATE INDEX index_Track_genre ON Track(genre);"#,
    r#"CREATE INDEX index_Track_artist ON Track(artist);"#,
    r#"CREATE INDEX index_Track_album ON Track(album);"#,
    r#"CREATE INDEX index_Track_key ON Track(key);"#,
    r#"CREATE INDEX index_PlaylistEntity_nextEntityId_listId ON PlaylistEntity(nextEntityId, listId);"#,
    r#"CREATE INDEX index_PreparelistEntity_trackId ON PreparelistEntity (trackId);"#,
    r#"CREATE TRIGGER trigger_after_insert_Pack_timestamp AFTER INSERT ON Pack FOR EACH ROW WHEN NEW.lastPackTime IS NULL BEGIN UPDATE Pack SET lastPackTime = strftime('%s') WHERE ROWID = NEW.ROWID; END;"#,
    r#"CREATE TRIGGER trigger_after_insert_Pack_changeLogId AFTER INSERT ON Pack FOR EACH ROW WHEN NEW.changeLogId = 0 BEGIN UPDATE Pack SET changeLogId = 1 WHERE ROWID = NEW.ROWID; END;"#,
    r#"CREATE VIEW ChangeLog (id, trackId) AS SELECT 0, 0 WHERE FALSE;"#,
    r#"CREATE TRIGGER trigger_after_insert_Track_check_id AFTER INSERT ON Track WHEN NEW.id <= (SELECT seq FROM sqlite_sequence WHERE name = 'Track') BEGIN SELECT RAISE(ABORT, 'Recycling deleted track id''s are not allowed'); END;"#,
    r#"CREATE TRIGGER trigger_after_update_Track_check_Id BEFORE UPDATE ON Track WHEN NEW.id <> OLD.id BEGIN SELECT RAISE(ABORT, 'Changing track id''s are not allowed'); END;"#,
    r#"CREATE TRIGGER trigger_after_insert_Track_fix_origin AFTER INSERT ON Track WHEN IFNULL(NEW.originTrackId, 0) = 0 OR IFNULL(NEW.originDatabaseUuid, '') = '' BEGIN UPDATE Track SET originTrackId = NEW.id, originDatabaseUuid = (SELECT uuid FROM Information) WHERE track.id = NEW.id; END;"#,
    r#"CREATE TRIGGER trigger_after_update_Track_fix_origin AFTER UPDATE ON Track WHEN IFNULL(NEW.originTrackId, 0) = 0 OR IFNULL(NEW.originDatabaseUuid, '') = '' BEGIN UPDATE Track SET originTrackId = NEW.id, originDatabaseUuid = (SELECT uuid FROM Information) WHERE track.id = NEW.id; END;"#,
    r#"CREATE TRIGGER trigger_after_update_Track_timestamp AFTER UPDATE OF length, bpm, year, filename, bitrate, bpmAnalyzed, albumArtId, title, artist, album, genre, comment, label, composer, remixer, key, rating, albumArt, fileType, isAnalyzed, isBeatgridLocked, trackData, overviewWaveformData, beatData, quickCues, loops, explicitLyrics, activeOnLoadLoops ON Track FOR EACH ROW BEGIN UPDATE Track SET lastEditTime = strftime('%s') WHERE ROWID=NEW.ROWID; END;"#,
    r#"CREATE VIEW PerformanceData AS SELECT id AS trackId, isAnalyzed, trackData, overviewWaveFormData, beatData, quickCues, loops, thirdPartySourceId, activeOnLoadLoops FROM Track;"#,
    r#"CREATE TRIGGER trigger_instead_update_thirdPartySourceId_PerformanceData INSTEAD OF UPDATE OF thirdPartySourceId ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET thirdPartySourceId = NEW.thirdPartySourceId WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_update_isAnalyzed_PerformanceData INSTEAD OF UPDATE OF isAnalyzed ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET isAnalyzed = NEW.isAnalyzed WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_update_trackData_PerformanceData INSTEAD OF UPDATE OF trackData ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET trackData = NEW.trackData WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_update_overviewWaveFormData_PerformanceData INSTEAD OF UPDATE OF overviewWaveFormData ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET overviewWaveFormData = NEW.overviewWaveFormData WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_update_beatData_PerformanceData INSTEAD OF UPDATE OF beatData ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET beatData = NEW.beatData WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_update_quickCues_PerformanceData INSTEAD OF UPDATE OF quickCues ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET quickCues = NEW.quickCues WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_update_loops_PerformanceData INSTEAD OF UPDATE OF loops ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET loops = NEW.loops WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_delete_PerformanceData INSTEAD OF DELETE ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET isAnalyzed = NULL, trackData = NULL, overviewWaveFormData = NULL, beatData = NULL, quickCues = NULL, loops = NULL, thirdPartySourceId = NULL WHERE Track.id = OLD.trackId; END;"#,
    r#"CREATE TRIGGER trigger_instead_update_activeOnLoadLoops_PerformanceData INSTEAD OF UPDATE OF activeOnLoadLoops ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET activeOnLoadLoops = NEW.activeOnLoadLoops WHERE Track.id = NEW.trackId; END;"#,
    r#"CREATE TRIGGER trigger_before_insert_List BEFORE INSERT ON Playlist FOR EACH ROW BEGIN UPDATE Playlist SET nextListId = -(1 + nextListId) WHERE nextListId = NEW.nextListId AND parentListId = NEW.parentListId; END;"#,
    r#"CREATE TRIGGER trigger_after_insert_List AFTER INSERT ON Playlist FOR EACH ROW BEGIN UPDATE Playlist SET nextListId = NEW.id WHERE nextListId = -(1 + NEW.nextListId) AND parentListId = NEW.parentListId; END;"#,
    r#"CREATE TRIGGER trigger_after_delete_List AFTER DELETE ON Playlist FOR EACH ROW BEGIN UPDATE Playlist SET nextListId = OLD.nextListId WHERE nextListId = OLD.id; DELETE FROM Playlist WHERE parentListId = OLD.id; END;"#,
    r#"CREATE TRIGGER trigger_after_update_isPersistParent AFTER UPDATE ON Playlist WHEN (old.isPersisted = 0 AND new.isPersisted = 1) OR (old.parentListId != new.parentListId AND new.isPersisted = 1) BEGIN UPDATE Playlist SET isPersisted = 1 WHERE id IN (SELECT parentListId FROM PlaylistAllParent WHERE id=new.id); END;"#,
    r#"CREATE TRIGGER trigger_after_update_isPersistChild AFTER UPDATE ON Playlist WHEN old.isPersisted = 1 AND new.isPersisted = 0 BEGIN UPDATE Playlist SET isPersisted = 0 WHERE id IN (SELECT childListId FROM PlaylistAllChildren WHERE id=new.id); END;"#,
    r#"CREATE TRIGGER trigger_after_insert_isPersist AFTER INSERT ON Playlist WHEN new.isPersisted = 1 BEGIN UPDATE Playlist SET isPersisted = 1 WHERE id IN (SELECT parentListId FROM PlaylistAllParent WHERE id=new.id); END;"#,
    r#"CREATE VIEW PlaylistAllParent AS WITH FindAllParent AS ( SELECT id, parentListId FROM Playlist UNION ALL SELECT recursiveCTE.id, Plist.parentListId FROM Playlist Plist INNER JOIN FindAllParent recursiveCTE ON recursiveCTE.parentListId = Plist.id ) SELECT * FROM FindAllParent;"#,
    r#"CREATE VIEW PlaylistAllChildren AS WITH FindAllChild AS ( SELECT id, id as childListId FROM Playlist UNION ALL SELECT recursiveCTE.id, Plist.id FROM Playlist Plist INNER JOIN FindAllChild recursiveCTE ON recursiveCTE.childListId = Plist.parentListId ) SELECT * FROM FindAllChild WHERE id <> childListId;"#,
    r#"CREATE VIEW PlaylistPath AS WITH RECURSIVE Heirarchy AS ( SELECT id AS child, parentListId AS parent, title AS name, 1 AS depth FROM Playlist UNION ALL SELECT child, parentListId AS parent, title AS name, h.depth + 1 AS depth FROM Playlist c JOIN Heirarchy h ON h.parent = c.id ORDER BY depth DESC ), OrderedList AS ( SELECT id , nextListId, 1 AS position FROM Playlist WHERE nextListId = 0 UNION ALL SELECT c.id , c.nextListId , l.position + 1 FROM Playlist c INNER JOIN OrderedList l ON c.nextListId = l.id ), NameConcat AS ( SELECT child AS id, GROUP_CONCAT(name ,';') || ';' AS path FROM ( SELECT child, name FROM Heirarchy ORDER BY depth DESC ) GROUP BY child ) SELECT id, path, ROW_NUMBER() OVER ( ORDER BY (SELECT COUNT(*) FROM (SELECT * FROM Heirarchy WHERE child = id) ) DESC, (SELECT position FROM OrderedList ol WHERE ol.id = c.id) ASC ) AS position FROM Playlist c LEFT JOIN NameConcat g USING (id);"#,
    r#"CREATE TRIGGER trigger_before_delete_PlaylistEntity BEFORE DELETE ON PlaylistEntity WHEN OLD.trackId > 0 BEGIN UPDATE PlaylistEntity SET nextEntityId = OLD.nextEntityId WHERE nextEntityId = OLD.id AND listId = OLD.listId; END;"#,
    r#"CREATE INDEX index_Track_bpmAnalyzed ON Track(CAST(bpmAnalyzed + 0.5 AS int));"#,
    r#"CREATE TRIGGER trigger_instead_insert_PerformanceData INSTEAD OF INSERT ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET isAnalyzed = IFNULL(NEW.isAnalyzed, isAnalyzed), trackData = NEW.trackData, overviewWaveFormData = NEW.overviewWaveFormData, beatData = NEW.beatData, quickCues = NEW.quickCues, loops = NEW.loops, thirdPartySourceId = NEW.thirdPartySourceId, activeOnLoadLoops = NEW.activeOnLoadLoops WHERE Track.id = NEW.trackId; END;"#,
    r#"INSERT INTO AlbumArt VALUES (1, '', NULL)"#,
];

/// Create the schema in an open connection.
pub fn create(connection: &Connection) -> Result<(), rusqlite::Error> {
    for statement in STATEMENTS {
        connection.execute_batch(statement)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_creates_the_tables_a_player_looks_for() {
        let connection = Connection::open_in_memory().unwrap();
        create(&connection).unwrap();

        let mut names: Vec<String> = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        names.retain(|name| !name.starts_with("sqlite_"));
        assert_eq!(
            names,
            vec![
                "AlbumArt",
                "Information",
                "Pack",
                "Playlist",
                "PlaylistEntity",
                "PreparelistEntity",
                "Smartlist",
                "Track"
            ]
        );

        // PerformanceData is a view over Track in this schema version, and the
        // one a player reads the analysis through.
        let views: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view' AND name = 'PerformanceData'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(views, 1);
    }
}
