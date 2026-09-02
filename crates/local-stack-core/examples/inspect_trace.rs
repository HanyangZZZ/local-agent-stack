use local_stack_core::TraceStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let requested = arguments
        .iter()
        .find(|argument| !argument.starts_with("--"));
    let store = TraceStore::discover(true, None, 4)?;
    let sessions = store.list_sessions().await?;
    let session_id = requested
        .cloned()
        .or_else(|| sessions.first().map(|session| session.id.clone()))
        .ok_or("no Harness sessions were found")?;
    let replay = store.load_session(&session_id).await?;
    let raw_records: usize = replay
        .events
        .iter()
        .map(|event| event.raw_event_count)
        .sum();
    println!(
        "{}\n{} logical operations · {} raw records · {} branches · {} leases · {} edges · {} telemetry samples",
        replay.session.title,
        replay.events.len(),
        raw_records,
        replay.branches.len(),
        replay.leases.len(),
        replay.edges.len(),
        replay.telemetry.len(),
    );
    if arguments.iter().any(|argument| argument == "--events") {
        for event in &replay.events {
            println!(
                "{:>3}  {:<28}  turn={:?} step={:?}  {} :: {}",
                event.index + 1,
                event.event_type,
                event.turn,
                event.step,
                event.label,
                event.message.replace(['\r', '\n'], " ")
            );
        }
    }
    if arguments.iter().any(|argument| argument == "--user-raw") {
        for event in &replay.events {
            if event.event_type == "user/message" {
                println!("{}", serde_json::to_string_pretty(&event.raw)?);
            }
        }
    }
    Ok(())
}
