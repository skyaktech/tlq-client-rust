use std::time::Duration;
use tlq_client::TlqClient;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TlqClient::builder()
        .host("localhost")
        .port(1337)
        .timeout_ms(5000)
        .max_retries(3)
        .build();

    let client = TlqClient::with_config(client);

    println!("Starting worker, polling for messages...");

    loop {
        match client.get_message().await {
            Ok(Some(message)) => {
                println!("Processing message {}: {}", message.id, message.body);
                
                match process_message(&message.body).await {
                    Ok(_) => {
                        println!("✅ Successfully processed message {}", message.id);
                        client.delete_message(message.id).await?;
                    }
                    Err(e) => {
                        println!("❌ Failed to process message {}: {}", message.id, e);
                        
                        if message.retry_count < 3 {
                            println!("Retrying message {} (attempt {})", message.id, message.retry_count + 1);
                            client.retry_message(message.id).await?;
                        } else {
                            println!("Message {} exceeded max retries, deleting", message.id);
                            client.delete_message(message.id).await?;
                        }
                    }
                }
            }
            Ok(None) => {
                println!("No messages available, waiting...");
                sleep(Duration::from_secs(1)).await;
            }
            Err(e) => {
                println!("Error fetching messages: {}", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn process_message(body: &str) -> Result<(), String> {
    println!("  Processing: {}", body);
    sleep(Duration::from_millis(100)).await;
    
    if body.contains("error") {
        Err("Message contains 'error'".to_string())
    } else {
        Ok(())
    }
}