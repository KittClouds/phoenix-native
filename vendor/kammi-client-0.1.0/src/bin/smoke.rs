use kammi_client::{KammiClient, Result};
use reqwest::Method;
use serde_json::json;

fn main() -> Result<()> {
    let client = KammiClient::new(&std::env::var("KAMMI_URL")?, &std::env::var("KAMMI_TOKEN")?)?;
    if let Ok(actor) = std::env::var("KAMMI_SMOKE_ACTOR") {
        let scope = std::env::var("KAMMI_SMOKE_SCOPE")?;
        let evidence = std::env::var("KAMMI_SMOKE_EVIDENCE")?;
        let record = client.memory_record(&json!({"kind":"OBSERVED", "scope":scope,
            "text":"Rust external SDK traced an immutable custody object",
            "actor_id":actor, "custody_refs":[evidence], "request_id":"rust-sdk-memory"}))?;
        let search = client.memory_search(&json!({"query":"immutable custody", "scope":scope,
            "actor_id":actor, "mode":"hybrid", "request_id":"rust-sdk-search"}))?;
        let id = record["memory_id"]
            .as_str()
            .ok_or("memory identity absent")?;
        let trace = client.call(
            Method::GET,
            &format!("/v1/memory/{id}/trace?actor_id={actor}"),
            None,
        )?;
        if search["results"].as_array().is_none_or(Vec::is_empty)
            || trace["references"][0]["verified"] != true
        {
            return Err("Rust SDK memory evidence qualification failed".into());
        }
        println!(
            "{}",
            json!({"status":"PASS", "record":record, "search":search, "trace":trace})
        );
    } else {
        println!("{}", client.status()?);
    }
    Ok(())
}
