use anyhow::Result;

use crate::cli::OntologyOpts;
use crate::ontology;

pub fn execute(opts: OntologyOpts) -> Result<()> {
    match opts.format.to_lowercase().as_str() {
        "json-ld" | "jsonld" => {
            let ont = ontology::build_ontology();
            let json = ontology::to_json_ld(&ont, opts.full);
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        "json" => {
            let ont = ontology::build_ontology();
            let json = ontology::to_json(&ont, opts.full);
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        "yaml" | "yml" => {
            let ont = ontology::build_ontology();
            let json = ontology::to_json(&ont, opts.full);
            println!("{}", serde_yaml::to_string(&json)?);
        }
        "openapi" | "openapi-json" => {
            let spec = ontology::openapi::generate_openapi_spec();
            println!("{}", serde_json::to_string_pretty(&spec)?);
        }
        "openapi-yaml" => {
            let spec = ontology::openapi::generate_openapi_spec();
            println!("{}", serde_yaml::to_string(&spec)?);
        }
        _ => {
            anyhow::bail!(
                "Unsupported ontology format: {}. Supported: json-ld, json, yaml, openapi, openapi-yaml",
                opts.format
            );
        }
    }

    Ok(())
}
