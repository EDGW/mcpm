//! Index concurrency checks covering optional busy updates and WAL-safe recovery.

use super::*;

#[test]
fn busy_updates_do_not_block_lookup_and_recovery_keeps_live_database() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join(".mcpm")).unwrap();
    let quick = ImoHash::new([0; 16]);
    let key = Blake3Hash::new([1; 32]);
    insert(temp.path(), &quick, &key).unwrap();
    let connection = connect(temp.path(), true).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(insert(temp.path(), &quick, &Blake3Hash::new([2; 32])).is_err());
    assert_eq!(candidates(temp.path(), &quick).unwrap(), vec![key]);
    assert!(replace(temp.path(), &[]).is_err());
    assert!(path(temp.path()).is_file());
    connection.execute_batch("ROLLBACK").unwrap();
    drop(connection);
    replace(temp.path(), &[(quick, key)]).unwrap();
    assert_eq!(candidates(temp.path(), &quick).unwrap(), vec![key]);
}
