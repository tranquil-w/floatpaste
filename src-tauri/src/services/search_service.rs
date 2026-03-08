use crate::{
    domain::{
        clip_item::{SearchQuery, SearchResult},
        error::AppError,
    },
    repository::sqlite_repository::SqliteRepository,
};

pub struct SearchService;

impl SearchService {
    pub fn search(
        repository: &SqliteRepository,
        query: SearchQuery,
    ) -> Result<SearchResult, AppError> {
        repository.search(query)
    }
}
