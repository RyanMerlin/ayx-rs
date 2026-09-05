use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use url::form_urlencoded::Serializer;

use crate::{
    DatasetFilter, OneDatasetsCommand, OneDatasetsImportedCommand, OneDatasetsWrangledCommand,
    cmd::RuntimeCtx,
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

fn datasets_filter_query(filters: &[DatasetFilter]) -> Vec<(&'static str, String)> {
    filters
        .iter()
        .map(|filter| ("datasetsFilter", filter.as_api_str().to_string()))
        .collect()
}

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneDatasetsCommand) -> Result<Envelope> {
    Ok(match command {
        OneDatasetsCommand::Create { profile, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = crate::load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "datasets",
                "create",
                "POST",
                "/v4/importedDatasets",
                true,
                &[],
                Some(payload),
            )?
        }
        OneDatasetsCommand::List {
            profile,
            datasets_filter,
            limit,
            offset,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let mut query = Vec::new();
            query.extend(datasets_filter_query(&datasets_filter));
            if let Some(limit) = limit {
                query.push(("limit", limit.to_string()));
            }
            if let Some(offset) = offset {
                query.push(("offset", offset.to_string()));
            }
            let endpoint = append_query("/v4/datasetLibrary", &query);
            one_api_live_request(&config, "datasets", "list", "GET", &endpoint, false, &[])?
        }
        OneDatasetsCommand::Count {
            profile,
            datasets_filter,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let mut query = Vec::new();
            if !datasets_filter.is_empty() {
                query.extend(datasets_filter_query(&datasets_filter));
            }
            let endpoint = append_query("/v4/datasetLibrary/count", &query);
            one_api_live_request(&config, "datasets", "count", "GET", &endpoint, false, &[])?
        }
        OneDatasetsCommand::Wrangled { command } => match command {
            OneDatasetsWrangledCommand::List {
                profile,
                limit,
                offset,
            } => {
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
            OneDatasetsWrangledCommand::Count { profile } => {
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
            OneDatasetsWrangledCommand::Detail { profile, id } => {
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
        OneDatasetsCommand::Imported { command } => match command {
            OneDatasetsImportedCommand::Detail { profile, id } => {
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

#[cfg(test)]
mod tests {
    use super::{DatasetFilter, datasets_filter_query};

    #[test]
    fn datasets_filter_query_serializes_single_value_as_one_pair() {
        assert_eq!(
            datasets_filter_query(&[DatasetFilter::All]),
            vec![("datasetsFilter", "all".to_string())]
        );
    }

    #[test]
    fn datasets_filter_query_serializes_multiple_values_as_repeated_pairs() {
        assert_eq!(
            datasets_filter_query(&[DatasetFilter::Imported, DatasetFilter::Recipe]),
            vec![
                ("datasetsFilter", "imported".to_string()),
                ("datasetsFilter", "recipe".to_string()),
            ]
        );
    }
}
