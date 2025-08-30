use tlq_client::TlqClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TlqClient::new("localhost", 1337)?;

    println!("Checking server health...");
    if client.health_check().await? {
        println!("✅ Server is healthy!");
    } else {
        println!("❌ Server health check failed");
        return Ok(());
    }

    println!("\nAdding a message to the queue...");
    let message = client.add_message("Hello, TLQ!").await?;
    println!("Added message with ID: {}", message.id);

    println!("\nRetrieving messages from the queue...");
    let messages = client.get_messages(1).await?;
    for msg in &messages {
        println!("Got message: {} - {}", msg.id, msg.body);
    }

    if let Some(msg) = messages.first() {
        println!("\nDeleting message {}...", msg.id);
        let deleted = client.delete_message(msg.id).await?;
        println!("Deleted {} message(s)", deleted);
    }

    Ok(())
}