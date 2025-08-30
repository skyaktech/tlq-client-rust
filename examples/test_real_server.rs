use tlq_client::TlqClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TlqClient::new("localhost", 1337)?;

    println!("🔍 Testing health check...");
    let is_healthy = client.health_check().await?;
    println!("Health check result: {}", is_healthy);

    println!("\n📝 Adding a message...");
    let message = client.add_message("Test message from Rust client").await?;
    println!("Added message: {:?}", message);

    println!("\n📥 Getting messages...");
    let messages = client.get_messages(1).await?;
    println!("Retrieved {} messages:", messages.len());
    for msg in &messages {
        println!("  - Message: {:?}", msg);
    }

    if let Some(msg) = messages.first() {
        println!("\n🗑️ Deleting message {}...", msg.id);
        let delete_result = client.delete_message(msg.id).await?;
        println!("Delete result: {}", delete_result);
    }

    println!("\n🧹 Purging queue...");
    let purge_result = client.purge_queue().await?;
    println!("Purge result: {}", purge_result);

    println!("\n✅ All operations completed successfully!");
    Ok(())
}