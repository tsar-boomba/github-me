use once_cell::sync::{Lazy, OnceCell};

pub static BUCKET_NAME: Lazy<String> = Lazy::new(|| std::env::var("BUCKET_NAME").unwrap());
const TOTAL_STATS_OBJ_NAME: &str = "total-stats.json";
const PER_REPO_OBJ_NAME: &str = "per-repo-stats.json";
static CLIENT: OnceCell<aws_sdk_s3::Client> = OnceCell::new();

async fn get_init_client() -> &'static aws_sdk_s3::Client {
    if CLIENT.get().is_none() {
        let sdk_config = aws_config::from_env().load().await;
        // Could fail if someone else set it between these statements (shouldn't happen, but being pedantic)
        CLIENT.set(aws_sdk_s3::Client::new(&sdk_config)).ok();
    }

    CLIENT.get().unwrap()
}

pub async fn save_stats(total_stats: &str, per_repo_stats: &str) -> Result<(), aws_sdk_s3::Error> {
    #[cfg(not(debug_assertions))]
    {
        let client = get_init_client().await;

        client
            .put_object()
            .bucket(&*BUCKET_NAME)
            .key(TOTAL_STATS_OBJ_NAME)
            .body(total_stats.as_bytes().to_vec().into())
            .send()
            .await?;

        client
            .put_object()
            .bucket(&*BUCKET_NAME)
            .key(PER_REPO_OBJ_NAME)
            .body(per_repo_stats.as_bytes().to_vec().into())
            .send()
            .await?;
    }

    #[cfg(debug_assertions)]
    {
        std::fs::write(TOTAL_STATS_OBJ_NAME, total_stats).unwrap();
        std::fs::write(PER_REPO_OBJ_NAME, per_repo_stats).unwrap();
    }

    Ok(())
}

pub async fn get_total_stats() -> Result<Vec<u8>, aws_sdk_s3::Error> {
	#[cfg(not(debug_assertions))]
    {
        let client = get_init_client().await;

        let total = client
            .get_object()
            .bucket(&*BUCKET_NAME)
            .key(TOTAL_STATS_OBJ_NAME)
            .send()
            .await?
            .body
            .collect()
            .await
            .unwrap()
            .to_vec();

		return Ok(total)
    }

    #[cfg(debug_assertions)]
    {
        let total = std::fs::read(TOTAL_STATS_OBJ_NAME).unwrap();

		return Ok(total)
    }
}

pub async fn get_per_repo_stats() -> Result<Vec<u8>, aws_sdk_s3::Error> {
	#[cfg(not(debug_assertions))]
    {
        let client = get_init_client().await;

        let per_repo = client
            .get_object()
            .bucket(&*BUCKET_NAME)
            .key(PER_REPO_OBJ_NAME)
            .send()
            .await?
            .body
            .collect()
            .await
            .unwrap()
            .to_vec();

		return Ok(per_repo)
    }

    #[cfg(debug_assertions)]
    {
        let per_repo = std::fs::read(PER_REPO_OBJ_NAME).unwrap();

		return Ok(per_repo)
    }
}
