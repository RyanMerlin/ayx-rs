use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::one_api_live_request;
use url::form_urlencoded::Serializer;

use crate::{
    OneDatasetsCommand, OneDatasetsImportedCommand, OneDatasetsWrangledCommand, cmd::RuntimeCtx,
};

fn append_query(endpoint: &str, query: &[(&str, String)]) -> String {
    if query.is_empty() {
        return endpoint.to_string();
    }
    let mut serializer = Serializer::new(String::new());
    for (key, value) in query {
        serializer.append_pair(key, value);
    }
    format!("{endpoint}?{}", serializer.finish())
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneDatasetsCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok("one datasets commands available: list, count, wrangled, imported"),
        Some(OneDatasetsCommand::List {
            profile,
            limit,
            offset,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let mut query = Vec::new();
            if let Some(limit) = limit {
                query.push(("limit", limit.to_string()));
            }
            if let Some(offset) = offset {
                query.push(("offset", offset.to_string()));
            }
            let endpoint = append_query("/v4/datasetLibrary", &query);
            one_api_live_request(&config, "datasets", "list", "GET", &endpoint, false, &[])?
        }
        Some(OneDatasetsCommand::Count { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "datasets",
                "count",
                "GET",
                "/v4/datasetLibrary/count",
                false,
                &[],
            )?
        }
        Some(OneDatasetsCommand::Wrangled { command }) => match command {
            None => Envelope::ok("one datasets wrangled commands available: list, count, detail"),
            Some(OneDatasetsWrangledCommand::List {
                profile,
                limit,
                offset,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let mut query = Vec::new();
                if let Some(limit) = limit {
                    query.push(("limit", limit.to_string()));
                }
                if let Some(offset) = offset {
                    query.push(("offset", offset.to_string()));
                }
                let endpoint = append_query("/v4/wrangledDatasets", &query);
                one_api_live_request(
                    &config,
                    "datasets",
                    "wrangled-list",
                    "GET",
                    &endpoint,
                    false,
                    &[],
                )?
            }
            Some(OneDatasetsWrangledCommand::Count { profile }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "datasets",
                    "wrangled-count",
                    "GET",
                    "/v4/wrangledDatasets/count",
                    false,
                    &[],
                )?
            }
            Some(OneDatasetsWrangledCommand::Detail { profile, id }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "datasets",
                    "wrangled-detail",
                    "GET",
                    "/v4/wrangledDatasets/{id}",
                    false,
                    &[("id", id.as_str())],
                )?
            }
        },
        Some(OneDatasetsCommand::Imported { command }) => match command {
            None => Envelope::ok("one datasets imported commands available: detail"),
            Some(OneDatasetsImportedCommand::Detail { profile, id }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "datasets",
                    "imported-detail",
                    "GET",
                    "/v4/importedDatasets/{id}",
                    false,
                    &[("id", id.as_str())],
                )?
            }
        },
    })
}
