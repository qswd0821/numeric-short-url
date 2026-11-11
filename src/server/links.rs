//! 링크 저장소. 생성(INSERT)과 조회(SELECT)만 존재한다. 수정·삭제 함수는 의도적으로 없다.

use sqlx::PgPool;

/// 원본 URL로 새 링크를 만들고, DB가 발급한 단축 번호를 돌려준다.
/// 같은 원본 URL이라도 호출할 때마다 새 단축 번호가 발급된다.
pub async fn insert(pool: &PgPool, original_url: &str) -> sqlx::Result<i64> {
    sqlx::query_scalar("INSERT INTO links (original_url) VALUES ($1) RETURNING id")
        .bind(original_url)
        .fetch_one(pool)
        .await
}

pub async fn find_original_url(pool: &PgPool, id: i64) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT original_url FROM links WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[sqlx::test(migrations = "./migrations")]
    async fn insert_then_find(pool: PgPool) {
        let id = insert(&pool, "https://example.org/a?b=1#c").await.unwrap();
        assert_eq!(find_original_url(&pool, id).await.unwrap().as_deref(), Some("https://example.org/a?b=1#c"));
        assert_eq!(find_original_url(&pool, id + 1000).await.unwrap(), None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn same_url_gets_new_number(pool: PgPool) {
        let a = insert(&pool, "https://example.org").await.unwrap();
        let b = insert(&pool, "https://example.org").await.unwrap();
        assert_ne!(a, b);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn concurrent_inserts_never_share_a_number(pool: PgPool) {
        let tasks = (0..50).map(|i| {
            let pool = pool.clone();
            tokio::spawn(async move {
                let url = format!("https://example.org/{i}");
                (insert(&pool, &url).await.unwrap(), url)
            })
        });
        let results: Vec<(i64, String)> = futures::future::join_all(tasks)
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect();

        let ids: HashSet<i64> = results.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), 50, "단축 번호가 중복 발급됨");
        for (id, url) in &results {
            assert_eq!(find_original_url(&pool, *id).await.unwrap().as_ref(), Some(url), "기존 링크가 덮어써짐");
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn database_rejects_update_delete_truncate(pool: PgPool) {
        let id = insert(&pool, "https://example.org").await.unwrap();

        let update = sqlx::query("UPDATE links SET original_url = 'https://evil.example' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await;
        assert!(update.is_err(), "UPDATE가 허용됨");

        let delete = sqlx::query("DELETE FROM links WHERE id = $1").bind(id).execute(&pool).await;
        assert!(delete.is_err(), "DELETE가 허용됨");

        let truncate = sqlx::query("TRUNCATE links").execute(&pool).await;
        assert!(truncate.is_err(), "TRUNCATE가 허용됨");

        let explicit_id = sqlx::query("INSERT INTO links (id, original_url) VALUES ($1, 'https://evil.example')")
            .bind(id + 1)
            .execute(&pool)
            .await;
        assert!(explicit_id.is_err(), "번호를 직접 지정한 INSERT가 허용됨");

        assert_eq!(find_original_url(&pool, id).await.unwrap().as_deref(), Some("https://example.org"));
    }
}
