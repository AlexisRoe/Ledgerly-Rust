//! Java counterpart: `shared/paging/KeysetPaging.java`.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgConnection, Postgres, QueryBuilder};

use super::Cursor;
use crate::id::TeamId;

/// One page, newest first. `next` is the cursor of the page after this one, `None` on the last page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysetPage<T> {
    pub items: Vec<T>,
    pub next: Option<Cursor>,
}

/// One page of a team's rows of `table`, ordered by `(created_at, id)` descending, after `after` if given.
///
/// Unlike offset paging it stays fast on a big table, and rows added while a client pages through it neither
/// repeat nor push others out of view: the next page starts strictly below the last row seen. `limit + 1` rows
/// are read to learn whether another page exists.
///
/// `table` must have `team_id`, `created_at` and `id` columns. It is the only text put into the statement; it
/// is a `&'static str`, a constant in the code, never input. Every value is a bound parameter.
///
/// # Errors
///
/// The database error, if the query fails.
pub async fn fetch<T>(
    connection: &mut PgConnection,
    table: &'static str,
    team: TeamId,
    after: Option<Cursor>,
    limit: u32,
    cursor_of: impl Fn(&T) -> Cursor,
) -> Result<KeysetPage<T>, sqlx::Error>
where
    T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
{
    let mut query = QueryBuilder::<Postgres>::new("select * from ");
    query.push(table).push(" where team_id = ").push_bind(team.as_uuid());
    if let Some(after) = after {
        query
            .push(" and (created_at, id) < (")
            .push_bind(after.timestamp)
            .push(", ")
            .push_bind(after.id)
            .push(")");
    }
    query
        .push(" order by created_at desc, id desc limit ")
        .push_bind(i64::from(limit) + 1);
    let mut items: Vec<T> = query.build_query_as().fetch_all(connection).await?;
    let wanted = usize::try_from(limit).unwrap_or(usize::MAX);
    if items.len() <= wanted {
        return Ok(KeysetPage { items, next: None });
    }
    items.truncate(wanted);
    let next = items.last().map(cursor_of);
    Ok(KeysetPage { items, next })
}
