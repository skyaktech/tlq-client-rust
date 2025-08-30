use tlq_client::TlqClient;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TlqClient::new("localhost", 1337)?;

    println!("Adding multiple messages to the queue...");
    let mut message_ids = Vec::new();
    
    for i in 1..=10 {
        let message = client.add_message(format!("Batch message #{}", i)).await?;
        println!("Added message {}: {}", i, message.id);
        message_ids.push(message.id);
    }

    println!("\nRetrieving messages in batches...");
    let batch_size = 5;
    let messages = client.get_messages(batch_size).await?;
    
    println!("Retrieved {} messages:", messages.len());
    for msg in &messages {
        println!("  - {}: {}", msg.id, msg.body);
    }

    println!("\nProcessing batch...");
    let mut successful_ids = Vec::new();
    let mut failed_ids = Vec::new();
    
    for msg in &messages {
        if msg.body.contains("#3") || msg.body.contains("#7") {
            println!("  ❌ Failed to process: {}", msg.body);
            failed_ids.push(msg.id);
        } else {
            println!("  ✅ Successfully processed: {}", msg.body);
            successful_ids.push(msg.id);
        }
    }

    if !successful_ids.is_empty() {
        println!("\nDeleting {} successful messages...", successful_ids.len());
        let deleted = client.delete_messages(&successful_ids).await?;
        println!("Deleted {} messages", deleted);
    }

    if !failed_ids.is_empty() {
        println!("\nRetrying {} failed messages...", failed_ids.len());
        let retried = client.retry_messages(&failed_ids).await?;
        println!("Retried {} messages", retried);
    }

    println!("\nPurging remaining messages...");
    let purged = client.purge_queue().await?;
    println!("Purged {} messages from queue", purged);

    Ok(())
}