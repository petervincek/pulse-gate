use deadpool_redis::Pool as RedisPool;

#[derive(Clone)]
pub struct AppState {
    pub redis_pool: RedisPool, // internally this uses smart pointers so it's easily clonable
    pub service_name: String,
}

impl AppState {
    pub fn new(redis_pool: RedisPool, service_name: impl Into<String>) -> Self {
        Self {
            redis_pool,
            service_name: service_name.into(),
        }
    }
}
