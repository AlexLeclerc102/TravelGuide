mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use common::TestApp;

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn members_list_hides_emails_from_non_organizers(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &editor, "editor").await;
    let uri = format!("/api/v1/trips/{id}/members");

    let as_orga = app.get(&uri, Some(&orga)).await;
    let list = as_orga.body.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["role"], "organizer");
    assert_eq!(list[1]["email"], "editor@example.com");

    let as_editor = app.get(&uri, Some(&editor)).await;
    assert!(as_editor
        .body
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["email"].is_null()));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn change_role_and_transfer_organization(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let bob = app.register("bob@example.com", "Bob").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &bob, "viewer").await;
    let bob_id = app.user_id(&bob).await;
    let orga_id = app.user_id(&orga).await;
    let member_uri = format!("/api/v1/trips/{id}/members/{bob_id}");

    // Un lecteur ne peut pas changer les rôles.
    let res = app
        .patch(
            &format!("/api/v1/trips/{id}/members/{orga_id}"),
            json!({ "role": "viewer" }),
            Some(&bob),
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);

    let res = app
        .patch(&member_uri, json!({ "role": "editor" }), Some(&orga))
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get(&format!("/api/v1/trips/{id}"), Some(&bob))
            .await
            .body["my_role"],
        "editor"
    );

    // L'organisateur ne change pas son propre rôle.
    let res = app
        .patch(
            &format!("/api/v1/trips/{id}/members/{orga_id}"),
            json!({ "role": "editor" }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);

    // Transfert.
    let res = app
        .patch(&member_uri, json!({ "role": "organizer" }), Some(&orga))
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get(&format!("/api/v1/trips/{id}"), Some(&bob))
            .await
            .body["my_role"],
        "organizer"
    );
    assert_eq!(
        app.get(&format!("/api/v1/trips/{id}"), Some(&orga))
            .await
            .body["my_role"],
        "editor"
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn leave_and_remove_members(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let bob = app.register("bob@example.com", "Bob").await;
    let carla = app.register("carla@example.com", "Carla").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &bob, "editor").await;
    app.join(&orga, &id, &carla, "editor").await;
    let (orga_id, bob_id, carla_id) = (
        app.user_id(&orga).await,
        app.user_id(&bob).await,
        app.user_id(&carla).await,
    );
    let m = |uid: &str| format!("/api/v1/trips/{id}/members/{uid}");

    // Un éditeur ne retire pas les autres.
    assert_eq!(
        app.delete(&m(&carla_id), Some(&bob)).await.status,
        StatusCode::FORBIDDEN
    );
    // L'organisateur ne quitte pas son voyage.
    assert_eq!(
        app.delete(&m(&orga_id), Some(&orga)).await.status,
        StatusCode::BAD_REQUEST
    );
    // Un membre quitte le voyage.
    assert_eq!(
        app.delete(&m(&bob_id), Some(&bob)).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&format!("/api/v1/trips/{id}"), Some(&bob))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // L'organisateur retire un membre.
    assert_eq!(
        app.delete(&m(&carla_id), Some(&orga)).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete(&m(&carla_id), Some(&orga)).await.status,
        StatusCode::NOT_FOUND
    );
}
