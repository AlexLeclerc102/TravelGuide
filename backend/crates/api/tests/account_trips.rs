mod common;

use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;

use common::{TestApp, PASSWORD};

async fn delete_account(app: &TestApp, session: &str, email: &str) {
    let res = app
        .request(
            Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": email, "password": PASSWORD })),
            Some(session),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT, "{:?}", res.body);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn deleting_organizer_transfers_to_oldest_editor(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let first_editor = app.register("first@example.com", "Première").await;
    let second_editor = app.register("second@example.com", "Seconde").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &viewer, "viewer").await;
    app.join(&orga, &id, &first_editor, "editor").await;
    app.join(&orga, &id, &second_editor, "editor").await;

    delete_account(&app, &orga, "orga@example.com").await;

    let uri = format!("/api/v1/trips/{id}");
    assert_eq!(
        app.get(&uri, Some(&first_editor)).await.body["my_role"],
        "organizer"
    );
    assert_eq!(
        app.get(&uri, Some(&second_editor)).await.body["my_role"],
        "editor"
    );
    assert_eq!(app.get(&uri, Some(&viewer)).await.body["member_count"], 3);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn deleting_organizer_without_editor_deletes_trip(db: PgPool) {
    let app = TestApp::new(db.clone());
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &viewer, "viewer").await;

    delete_account(&app, &orga, "orga@example.com").await;

    assert_eq!(
        app.get(&format!("/api/v1/trips/{id}"), Some(&viewer))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM trips")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn deleting_member_keeps_trip(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let bob = app.register("bob@example.com", "Bob").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &bob, "editor").await;
    delete_account(&app, &bob, "bob@example.com").await;
    let trip = app.get(&format!("/api/v1/trips/{id}"), Some(&orga)).await;
    assert_eq!(trip.body["member_count"], 1);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn export_lists_trip_memberships(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    app.create_trip(&orga, "Pura Vida").await;
    let export = app.get("/api/v1/me/export", Some(&orga)).await;
    assert_eq!(export.body["trips"][0]["trip_name"], "Pura Vida");
    assert_eq!(export.body["trips"][0]["role"], "organizer");
}
