use uuid::Uuid;

/// A new time-ordered id (UUIDv7). Ids created later sort after earlier ones.
pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}
