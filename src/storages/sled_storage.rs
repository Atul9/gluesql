use sled::{self, Config, Db, IVec};
use std::convert::TryFrom;
use thiserror::Error as ThisError;

use crate::{Error, MutResult, Result, Row, RowIter, Schema, Store, StoreError, StoreMut};

#[derive(ThisError, Debug)]
enum StorageError {
    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Sled(#[from] sled::Error),
    #[error(transparent)]
    Bincode(#[from] bincode::Error),
}

impl Into<Error> for StorageError {
    fn into(self) -> Error {
        use StorageError::*;

        match self {
            Sled(e) => Error::Storage(Box::new(e)),
            Bincode(e) => Error::Storage(e),
            Store(e) => e.into(),
        }
    }
}

macro_rules! try_into {
    ($expr: expr) => {
        $expr.map_err(|e| {
            let e: StorageError = e.into();
            let e: Error = e.into();

            e
        })?
    };
}

macro_rules! try_self {
    ($self: expr, $expr: expr) => {
        match $expr {
            Err(e) => {
                let e: StorageError = e.into();
                let e: Error = e.into();

                return Err(($self, e));
            }
            Ok(v) => v,
        }
    };
}

#[derive(Debug)]
pub struct SledStorage {
    tree: Db,
}

impl SledStorage {
    pub fn new(filename: &str) -> Result<Self> {
        let tree = try_into!(sled::open(filename));

        Ok(Self { tree })
    }
}

impl TryFrom<Config> for SledStorage {
    type Error = Error;

    fn try_from(config: Config) -> Result<Self> {
        let tree = try_into!(config.open());

        Ok(Self { tree })
    }
}

impl StoreMut<IVec> for SledStorage {
    fn generate_id(self, table_name: &str) -> MutResult<Self, IVec> {
        let id = try_self!(self, self.tree.generate_id());
        let id = format!("data/{}/{}", table_name, id);

        Ok((self, IVec::from(id.as_bytes())))
    }

    fn insert_schema(self, schema: &Schema) -> MutResult<Self, ()> {
        let key = format!("schema/{}", schema.table_name);
        let key = key.as_bytes();
        let value = try_self!(self, bincode::serialize(schema));

        try_self!(self, self.tree.insert(key, value));

        Ok((self, ()))
    }

    fn delete_schema(self, table_name: &str) -> MutResult<Self, ()> {
        let prefix = format!("data/{}/", table_name);
        let tree = &self.tree;

        for item in tree.scan_prefix(prefix.as_bytes()) {
            let (key, _) = try_self!(self, item);

            try_self!(self, tree.remove(key));
        }

        let key = format!("schema/{}", table_name);
        try_self!(self, tree.remove(key));

        Ok((self, ()))
    }

    fn insert_data(self, key: &IVec, row: Row) -> MutResult<Self, Row> {
        let value = try_self!(self, bincode::serialize(&row));

        try_self!(self, self.tree.insert(key, value));

        Ok((self, row))
    }

    fn delete_data(self, key: &IVec) -> MutResult<Self, ()> {
        try_self!(self, self.tree.remove(key));

        Ok((self, ()))
    }
}

impl Store<IVec> for SledStorage {
    fn fetch_schema(&self, table_name: &str) -> Result<Schema> {
        let key = format!("schema/{}", table_name);
        let key = key.as_bytes();
        let value = try_into!(self.tree.get(&key));
        let value = value.ok_or(StoreError::SchemaNotFound)?;
        let statement = try_into!(bincode::deserialize(&value));

        Ok(statement)
    }

    fn scan_data(&self, table_name: &str) -> Result<RowIter<IVec>> {
        let prefix = format!("data/{}/", table_name);

        let result_set = self.tree.scan_prefix(prefix.as_bytes()).map(move |item| {
            let (key, value) = try_into!(item);
            let value = try_into!(bincode::deserialize(&value));

            Ok((key, value))
        });

        Ok(Box::new(result_set))
    }
}
