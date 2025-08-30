use std::time::Duration;

/// Configuration settings for TLQ client connections.
///
/// This struct contains all the configurable parameters for connecting to and
/// interacting with a TLQ server, including network settings and retry behavior.
///
/// # Default Values
///
/// - `host`: "localhost"
/// - `port`: 1337
/// - `timeout`: 30 seconds
/// - `max_retries`: 3
/// - `retry_delay`: 100 milliseconds (base delay for exponential backoff)
///
/// # Examples
///
/// ```
/// use tlq_client::{Config, ConfigBuilder};
/// use std::time::Duration;
///
/// // Using defaults
/// let default_config = Config::default();
/// assert_eq!(default_config.host, "localhost");
/// assert_eq!(default_config.port, 1337);
///
/// // Using builder pattern
/// let custom_config = ConfigBuilder::new()
///     .host("queue.example.com")
///     .port(8080)
///     .timeout(Duration::from_secs(60))
///     .max_retries(5)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct Config {
    /// Hostname or IP address of the TLQ server
    pub host: String,
    /// Port number of the TLQ server
    pub port: u16,
    /// Maximum time to wait for a single request to complete
    pub timeout: Duration,
    /// Maximum number of retry attempts for failed operations
    pub max_retries: u32,
    /// Base delay between retry attempts (exponential backoff multiplier)
    pub retry_delay: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 1337,
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(100),
        }
    }
}

/// Builder for creating [`Config`] instances with custom settings.
///
/// `ConfigBuilder` provides a fluent interface for constructing [`Config`] objects
/// with specific settings. All methods return `self` to enable method chaining.
/// Call [`build()`](Self::build) to create the final [`Config`] instance.
///
/// # Examples
///
/// ```
/// use tlq_client::ConfigBuilder;
/// use std::time::Duration;
///
/// let config = ConfigBuilder::new()
///     .host("queue.example.com")
///     .port(8080)
///     .timeout_ms(5000)          // 5 second timeout
///     .max_retries(2)            // Only retry twice
///     .retry_delay_ms(250)       // 250ms base delay
///     .build();
///
/// assert_eq!(config.host, "queue.example.com");
/// assert_eq!(config.port, 8080);
/// assert_eq!(config.timeout, Duration::from_millis(5000));
/// ```
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Creates a new `ConfigBuilder` with default settings.
    ///
    /// Equivalent to `Config::default()` but allows method chaining.
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    ///
    /// let builder = ConfigBuilder::new();
    /// let config = builder.build();
    /// ```
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Sets the TLQ server hostname or IP address.
    ///
    /// # Arguments
    ///
    /// * `host` - Any type that can be converted to `String`
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    ///
    /// let config = ConfigBuilder::new()
    ///     .host("queue.example.com")
    ///     .build();
    /// assert_eq!(config.host, "queue.example.com");
    /// ```
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.config.host = host.into();
        self
    }

    /// Sets the TLQ server port number.
    ///
    /// # Arguments
    ///
    /// * `port` - Port number (1-65535)
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    ///
    /// let config = ConfigBuilder::new()
    ///     .port(8080)
    ///     .build();
    /// assert_eq!(config.port, 8080);
    /// ```
    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    /// Sets the request timeout duration.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum time to wait for each request
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    /// use std::time::Duration;
    ///
    /// let config = ConfigBuilder::new()
    ///     .timeout(Duration::from_secs(60))
    ///     .build();
    /// assert_eq!(config.timeout, Duration::from_secs(60));
    /// ```
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Sets the request timeout in milliseconds.
    ///
    /// Convenience method equivalent to `timeout(Duration::from_millis(ms))`.
    ///
    /// # Arguments
    ///
    /// * `ms` - Timeout in milliseconds
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    /// use std::time::Duration;
    ///
    /// let config = ConfigBuilder::new()
    ///     .timeout_ms(5000)  // 5 seconds
    ///     .build();
    /// assert_eq!(config.timeout, Duration::from_millis(5000));
    /// ```
    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.config.timeout = Duration::from_millis(ms);
        self
    }

    /// Sets the maximum number of retry attempts.
    ///
    /// When a retryable error occurs, the client will retry the operation
    /// up to this many times before giving up.
    ///
    /// # Arguments
    ///
    /// * `retries` - Maximum retry attempts (0 disables retries)
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    ///
    /// let config = ConfigBuilder::new()
    ///     .max_retries(5)
    ///     .build();
    /// assert_eq!(config.max_retries, 5);
    /// ```
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.config.max_retries = retries;
        self
    }

    /// Sets the base retry delay duration.
    ///
    /// The actual delay between retries uses exponential backoff:
    /// delay = base_delay × 2^attempt_number
    ///
    /// # Arguments
    ///
    /// * `delay` - Base delay for exponential backoff
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    /// use std::time::Duration;
    ///
    /// let config = ConfigBuilder::new()
    ///     .retry_delay(Duration::from_millis(500))
    ///     .build();
    /// assert_eq!(config.retry_delay, Duration::from_millis(500));
    /// ```
    pub fn retry_delay(mut self, delay: Duration) -> Self {
        self.config.retry_delay = delay;
        self
    }

    /// Sets the base retry delay in milliseconds.
    ///
    /// Convenience method equivalent to `retry_delay(Duration::from_millis(ms))`.
    ///
    /// # Arguments
    ///
    /// * `ms` - Base delay in milliseconds
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    /// use std::time::Duration;
    ///
    /// let config = ConfigBuilder::new()
    ///     .retry_delay_ms(200)  // 200ms base delay
    ///     .build();
    /// assert_eq!(config.retry_delay, Duration::from_millis(200));
    /// ```
    pub fn retry_delay_ms(mut self, ms: u64) -> Self {
        self.config.retry_delay = Duration::from_millis(ms);
        self
    }

    /// Builds and returns the final [`Config`] instance.
    ///
    /// Consumes the builder and returns a [`Config`] with all the
    /// specified settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::ConfigBuilder;
    ///
    /// let config = ConfigBuilder::new()
    ///     .host("localhost")
    ///     .port(1337)
    ///     .max_retries(3)
    ///     .build();
    /// // Use config...
    /// ```
    pub fn build(self) -> Config {
        self.config
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
