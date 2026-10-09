use reqwest::Method;

use super::LIST_FIELDS;
use crate::{BaseItem, Client, ItemKind, ItemsPage, Result, UserData, UserView};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SortBy {
    #[default]
    Name,
    DateAdded,
    ReleaseDate,
    Rating,
    Random,
}

impl SortBy {
    fn param(self) -> &'static str {
        match self {
            SortBy::Name => "SortName",
            SortBy::DateAdded => "DateCreated,SortName",
            SortBy::ReleaseDate => "PremiereDate,ProductionYear,SortName",
            SortBy::Rating => "CommunityRating,SortName",
            SortBy::Random => "Random",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SortOrder {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ItemFilter {
    #[default]
    All,
    Unplayed,
    Favorites,
}

/// A page of a library listing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemsQuery {
    pub parent_id: Option<String>,
    /// Search the whole tree under `parent_id`; a collection's own titles
    /// need `false`.
    pub recursive: bool,
    pub include_item_types: Vec<ItemKind>,
    /// Matches names (as you type).
    pub search_term: Option<String>,
    /// Genre names; an item matches any of them.
    pub genres: Vec<String>,
    /// Items these people appear in (cast or crew).
    pub person_ids: Vec<String>,
    pub sort_by: SortBy,
    pub sort_order: SortOrder,
    pub filter: ItemFilter,
    pub start_index: u32,
    pub limit: u32,
}

impl Default for ItemsQuery {
    fn default() -> Self {
        Self {
            parent_id: None,
            recursive: true,
            include_item_types: Vec::new(),
            search_term: None,
            genres: Vec::new(),
            person_ids: Vec::new(),
            sort_by: SortBy::Name,
            sort_order: SortOrder::Ascending,
            filter: ItemFilter::All,
            start_index: 0,
            limit: 100,
        }
    }
}

fn join_kinds(kinds: &[ItemKind]) -> String {
    kinds
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

impl Client {
    fn user_query(&self) -> Result<Vec<(&'static str, String)>> {
        Ok(vec![("userId", self.require_user_id()?.to_string())])
    }

    pub async fn user_views(&self) -> Result<Vec<UserView>> {
        let page: ItemsPage<UserView> = self.get_json("UserViews", &self.user_query()?).await?;
        Ok(page.items)
    }

    pub async fn resume_items(&self, limit: u32) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.extend([
            ("mediaTypes", "Video".to_string()),
            ("limit", limit.to_string()),
            ("fields", LIST_FIELDS.to_string()),
            ("enableTotalRecordCount", "false".to_string()),
        ]);
        let page: ItemsPage<BaseItem> = self.get_json("UserItems/Resume", &query).await?;
        Ok(page.items)
    }

    /// Next episodes to watch. With `series_id`, only that show's next episode.
    /// `include_resumable` also returns half-watched episodes (wanted on a
    /// series page; unwanted on Home where Continue Watching shows them).
    pub async fn next_up(
        &self,
        series_id: Option<&str>,
        limit: u32,
        include_resumable: bool,
    ) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.extend([
            ("limit", limit.to_string()),
            ("fields", LIST_FIELDS.to_string()),
            ("enableResumable", include_resumable.to_string()),
            ("enableTotalRecordCount", "false".to_string()),
        ]);
        if let Some(series_id) = series_id {
            query.push(("seriesId", series_id.to_string()));
        }
        let page: ItemsPage<BaseItem> = self.get_json("Shows/NextUp", &query).await?;
        Ok(page.items)
    }

    /// Recently added items in a library (returns a bare array, not a page).
    pub async fn latest(&self, parent_id: &str, limit: u32) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.extend([
            ("parentId", parent_id.to_string()),
            ("limit", limit.to_string()),
            ("fields", LIST_FIELDS.to_string()),
        ]);
        self.get_json("Items/Latest", &query).await
    }

    pub async fn items(&self, items_query: &ItemsQuery) -> Result<ItemsPage<BaseItem>> {
        let mut query = self.user_query()?;
        query.push(("recursive", items_query.recursive.to_string()));
        if let Some(parent) = &items_query.parent_id {
            query.push(("parentId", parent.clone()));
        }
        if !items_query.include_item_types.is_empty() {
            query.push((
                "includeItemTypes",
                join_kinds(&items_query.include_item_types),
            ));
        }
        if let Some(term) = items_query.search_term.as_deref().map(str::trim)
            && !term.is_empty()
        {
            query.push(("searchTerm", term.to_string()));
        }
        if !items_query.genres.is_empty() {
            query.push(("genres", items_query.genres.join("|")));
        }
        if !items_query.person_ids.is_empty() {
            query.push(("personIds", items_query.person_ids.join(",")));
        }
        query.push(("sortBy", items_query.sort_by.param().to_string()));
        let order = match items_query.sort_order {
            SortOrder::Ascending => "Ascending",
            SortOrder::Descending => "Descending",
        };
        query.push(("sortOrder", order.to_string()));
        match items_query.filter {
            ItemFilter::All => {}
            ItemFilter::Unplayed => query.push(("filters", "IsUnplayed".to_string())),
            ItemFilter::Favorites => query.push(("filters", "IsFavorite".to_string())),
        }
        query.extend([
            ("startIndex", items_query.start_index.to_string()),
            ("limit", items_query.limit.to_string()),
            ("fields", LIST_FIELDS.to_string()),
            ("imageTypeLimit", "1".to_string()),
        ]);
        self.get_json("Items", &query).await
    }

    /// Full details, including media sources, people and user data.
    pub async fn item(&self, id: &str) -> Result<BaseItem> {
        self.get_json(&format!("Items/{id}"), &self.user_query()?)
            .await
    }

    pub async fn seasons(&self, series_id: &str) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.push(("fields", LIST_FIELDS.to_string()));
        let page: ItemsPage<BaseItem> = self
            .get_json(&format!("Shows/{series_id}/Seasons"), &query)
            .await?;
        Ok(page.items)
    }

    pub async fn episodes(&self, series_id: &str, season_id: &str) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.extend([
            ("seasonId", season_id.to_string()),
            ("fields", LIST_FIELDS.to_string()),
        ]);
        let page: ItemsPage<BaseItem> = self
            .get_json(&format!("Shows/{series_id}/Episodes"), &query)
            .await?;
        Ok(page.items)
    }

    /// People (cast and crew) whose names match `search_term`.
    pub async fn persons(&self, search_term: &str, limit: u32) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.extend([
            ("searchTerm", search_term.trim().to_string()),
            ("limit", limit.to_string()),
            ("fields", "PrimaryImageAspectRatio".to_string()),
            ("enableTotalRecordCount", "false".to_string()),
        ]);
        let page: ItemsPage<BaseItem> = self.get_json("Persons", &query).await?;
        Ok(page.items)
    }

    /// Genres used by `kinds` of items, in one library or (without
    /// `parent_id`) all of them.
    pub async fn genres(
        &self,
        parent_id: Option<&str>,
        kinds: &[ItemKind],
    ) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.extend([
            ("recursive", "true".to_string()),
            ("sortBy", "SortName".to_string()),
            ("enableTotalRecordCount", "false".to_string()),
        ]);
        if let Some(parent) = parent_id {
            query.push(("parentId", parent.to_string()));
        }
        if !kinds.is_empty() {
            query.push(("includeItemTypes", join_kinds(kinds)));
        }
        let page: ItemsPage<BaseItem> = self.get_json("Genres", &query).await?;
        Ok(page.items)
    }

    pub async fn similar(&self, id: &str, limit: u32) -> Result<Vec<BaseItem>> {
        let mut query = self.user_query()?;
        query.extend([
            ("limit", limit.to_string()),
            ("fields", LIST_FIELDS.to_string()),
        ]);
        let page: ItemsPage<BaseItem> = self
            .get_json(&format!("Items/{id}/Similar"), &query)
            .await?;
        Ok(page.items)
    }

    pub async fn set_played(&self, id: &str, played: bool) -> Result<UserData> {
        self.user_data_toggle("UserPlayedItems", id, played).await
    }

    pub async fn set_favorite(&self, id: &str, favorite: bool) -> Result<UserData> {
        self.user_data_toggle("UserFavoriteItems", id, favorite)
            .await
    }

    async fn user_data_toggle(&self, endpoint: &str, id: &str, on: bool) -> Result<UserData> {
        let method = if on { Method::POST } else { Method::DELETE };
        let request = self
            .request(method, &format!("{endpoint}/{id}"))
            .query(&self.user_query()?);
        self.send_json(request).await
    }
}
