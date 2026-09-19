// SPDX-License-Identifier: MPL-2.0
mod postgres;
mod sqlite;
use crate::{Result, profiles::Profile, query, results::Data};
use tokio_util::sync::CancellationToken;
pub enum Database {
    Sqlite(sqlite::Sqlite),
    Postgres(postgres::Postgres),
}
#[derive(Clone, Debug)]
pub struct Table {
    pub schema: String,
    pub name: String,
    pub kind: String,
}
impl Database {
    pub fn usable(&self) -> bool {
        match self {
            Self::Sqlite(_) => true,
            Self::Postgres(p) => p.usable(),
        }
    }
    pub async fn open(profile: &Profile, writable: bool, password: String) -> Result<Self> {
        profile.validate()?;
        match profile {
            Profile::Sqlite { path, .. } => Ok(Self::Sqlite(
                sqlite::Sqlite::open(path.clone(), writable).await?,
            )),
            Profile::Postgres { .. } => Ok(Self::Postgres(
                postgres::Postgres::open(profile, password).await?,
            )),
        }
    }
    pub async fn execute(
        &self,
        sql: String,
        write: bool,
        cancel: CancellationToken,
        seconds: u64,
    ) -> Result<Data> {
        query::validate(&sql, matches!(self, Self::Postgres(_)))?;
        self.raw(sql, write, cancel, seconds).await
    }
    async fn raw(
        &self,
        sql: String,
        write: bool,
        cancel: CancellationToken,
        seconds: u64,
    ) -> Result<Data> {
        match self {
            Self::Sqlite(db) => {
                if write && !db.writable {
                    return Err("Read-only connection".into());
                }
                db.execute(sql, write, cancel, seconds).await
            }
            Self::Postgres(db) => db.execute(sql, write, cancel, seconds).await,
        }
    }
    pub async fn settle(&self, commit: bool) -> Result<()> {
        match self {
            Self::Sqlite(db) => db.settle(commit).await,
            Self::Postgres(db) => db.settle(commit).await,
        }
    }
    pub async fn catalog(&self, system: bool, cancel: CancellationToken) -> Result<Vec<Table>> {
        let sql = match self {
            Self::Sqlite(_) => format!(
                "SELECT 'main', name, type FROM sqlite_schema WHERE type IN ('table','view') {} ORDER BY name",
                if system {
                    ""
                } else {
                    "AND name NOT LIKE 'sqlite_%'"
                }
            ),
            Self::Postgres(_) => format!(
                "SELECT n.nspname, c.relname, CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view' ELSE 'table' END FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind IN ('r','p','v','m','f') {} ORDER BY n.nspname,c.relname",
                if system {
                    ""
                } else {
                    "AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%'"
                }
            ),
        };
        let data = self.raw(sql, false, cancel, 30).await?;
        if data.truncated {
            return Err(
                "Catalog exceeds 1,000 objects; use a restricted database/schema role".into(),
            );
        }
        Ok(data
            .rows
            .into_iter()
            .filter_map(|r| {
                Some(Table {
                    schema: r.first()?.text.clone()?,
                    name: r.get(1)?.text.clone()?,
                    kind: r.get(2)?.text.clone()?,
                })
            })
            .collect())
    }
    pub async fn browse(&self, t: &Table, page: usize, cancel: CancellationToken) -> Result<Data> {
        self.browse_with(t, page, &crate::browse::Browse::default(), cancel)
            .await
    }
    pub async fn browse_keys(&self, t: &Table, cancel: CancellationToken) -> Result<Vec<String>> {
        let keys_sql = match self {
            Self::Sqlite(_) => format!(
                "SELECT name FROM pragma_table_info({}) WHERE pk>0 ORDER BY pk",
                query::literal(&t.name)
            ),
            Self::Postgres(_) => format!(
                "SELECT a.attname FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(attnum,ord) ON true JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.attnum WHERE i.indisprimary AND n.nspname={} AND c.relname={} ORDER BY k.ord",
                query::literal(&t.schema),
                query::literal(&t.name)
            ),
        };
        let keys = self.raw(keys_sql, false, cancel.clone(), 30).await?;
        Ok(keys
            .rows
            .iter()
            .filter_map(|r| r.first()?.text.clone())
            .collect())
    }
    pub async fn browse_with(
        &self,
        t: &Table,
        page: usize,
        browse: &crate::browse::Browse,
        cancel: CancellationToken,
    ) -> Result<Data> {
        let keys = self.browse_keys(t, cancel.clone()).await?;
        // Retain all fields for record inspection; selected ordinals affect display and export.
        let mut full = browse.clone();
        full.selected.clear();
        let (sql, parameters) =
            full.compile(t, page, &keys, matches!(self, Self::Postgres(_)), false)?;
        let mut data = match self {
            Self::Sqlite(db) => db.execute_bound(sql, parameters, false, cancel, 30).await,
            Self::Postgres(db) => db.execute_bound(sql, parameters, false, cancel, 30).await,
        }?;
        data.order_keys = keys;
        Ok(data)
    }
    pub async fn schema(&self, t: &Table, cancel: CancellationToken) -> Result<Data> {
        let sql = match self {
            Self::Sqlite(_) => format!(
                "SELECT 'column' AS kind, name, type AS definition, CASE WHEN pk>0 THEN 'primary key #'||pk ELSE '' END AS key, \"notnull\" AS required, dflt_value AS default_value FROM pragma_table_info({0}) UNION ALL SELECT 'index', name, COALESCE(sql,'automatic'),'',NULL,NULL FROM sqlite_schema WHERE type='index' AND tbl_name={0} UNION ALL SELECT 'foreign key', \"from\", \"table\"||'('||\"to\"||')', 'on update '||on_update||' on delete '||on_delete,NULL,NULL FROM pragma_foreign_key_list({0})",
                query::literal(&t.name)
            ),
            Self::Postgres(_) => format!(
                "SELECT 'column' AS kind, a.attname AS name, format_type(a.atttypid,a.atttypmod) AS definition, a.attnotnull::text AS required, pg_get_expr(d.adbin,d.adrelid) AS default_value FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum WHERE n.nspname={0} AND c.relname={1} AND a.attnum>0 AND NOT a.attisdropped UNION ALL SELECT 'constraint', con.conname, pg_get_constraintdef(con.oid),NULL,NULL FROM pg_constraint con JOIN pg_class c ON c.oid=con.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={0} AND c.relname={1} UNION ALL SELECT 'index',indexname,indexdef,NULL,NULL FROM pg_indexes WHERE schemaname={0} AND tablename={1}",
                query::literal(&t.schema),
                query::literal(&t.name)
            ),
        };
        self.raw(sql, false, cancel, 30).await
    }
}
