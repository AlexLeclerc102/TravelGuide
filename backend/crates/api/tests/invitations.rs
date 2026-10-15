mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use common::{session_token, TestApp};

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn guest_joins_with_name_and_email_only(db: PgPool) {
    let app = TestApp::new(db.clone());
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let id = app.create_trip(&orga, "Pura Vida").await;
    let token = app.invite(&orga, &id, "editor").await;

    let preview = app.get(&format!("/api/v1/invitations/{token}"), None).await;
    assert_eq!(preview.status, StatusCode::OK);
    assert_eq!(preview.body["trip_name"], "Pura Vida");
    assert_eq!(preview.body["invited_by"], "Orga");
    assert_eq!(preview.body["role"], "editor");

    let res = app
        .post(
            &format!("/api/v1/invitations/{token}/accept"),
            json!({ "display_name": "Lucía", "email": "lucia@example.com" }),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["trip_id"], id.as_str());
    assert_eq!(res.body["role"], "editor");
    assert_eq!(res.body["user"]["has_password"], false);
    assert_eq!(res.body["user"]["plan"], "free");

    let guest = session_token(&res).expect("session ouverte immédiatement");
    let trip = app.get(&format!("/api/v1/trips/{id}"), Some(&guest)).await;
    assert_eq!(trip.status, StatusCode::OK);
    assert_eq!(trip.body["my_role"], "editor");

    let uses: i32 = sqlx::query_scalar("SELECT uses FROM invitations")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(uses, 1);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn existing_email_must_log_in_first(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    app.register("ana@example.com", "Ana").await;
    let id = app.create_trip(&orga, "Voyage").await;
    let token = app.invite(&orga, &id, "viewer").await;

    let res = app
        .post(
            &format!("/api/v1/invitations/{token}/accept"),
            json!({ "display_name": "Imposteur", "email": "ANA@example.com" }),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.body["code"], "account_exists");
    assert!(session_token(&res).is_none());
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn accept_requires_identity_without_session(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let id = app.create_trip(&orga, "Voyage").await;
    let token = app.invite(&orga, &id, "viewer").await;
    let res = app
        .post(
            &format!("/api/v1/invitations/{token}/accept"),
            json!({}),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn revoked_expired_and_exhausted_invitations_are_refused(db: PgPool) {
    let app = TestApp::new(db.clone());
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let id = app.create_trip(&orga, "Voyage").await;
    let accept = |token: String, email: &'static str| {
        let app = &app;
        async move {
            app.post(
                &format!("/api/v1/invitations/{token}/accept"),
                json!({ "display_name": "Invité", "email": email }),
                None,
            )
            .await
        }
    };

    // Révoquée.
    let revoked = app.invite(&orga, &id, "viewer").await;
    let list = app
        .get(&format!("/api/v1/trips/{id}/invitations"), Some(&orga))
        .await;
    let inv_id = list.body[0]["id"].as_str().unwrap().to_owned();
    let res = app
        .delete(
            &format!("/api/v1/trips/{id}/invitations/{inv_id}"),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        accept(revoked.clone(), "a@example.com").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.get(&format!("/api/v1/invitations/{revoked}"), None)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    // Épuisée (max_uses = 1).
    let res = app
        .post(
            &format!("/api/v1/trips/{id}/invitations"),
            json!({ "role": "viewer", "max_uses": 1 }),
            Some(&orga),
        )
        .await;
    let single = res.body["url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        accept(single.clone(), "b@example.com").await.status,
        StatusCode::OK
    );
    assert_eq!(
        accept(single, "c@example.com").await.status,
        StatusCode::BAD_REQUEST
    );

    // Expirée.
    let expired = app.invite(&orga, &id, "viewer").await;
    sqlx::query("UPDATE invitations SET expires_at = now() - interval '1 second' WHERE revoked_at IS NULL AND max_uses IS NULL")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        accept(expired, "d@example.com").await.status,
        StatusCode::BAD_REQUEST
    );

    // Jeton inconnu.
    assert_eq!(
        accept("inconnu".into(), "e@example.com").await.status,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn invitation_validation(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let id = app.create_trip(&orga, "Voyage").await;
    let uri = format!("/api/v1/trips/{id}/invitations");
    for body in [
        json!({ "role": "organizer" }),
        json!({ "role": "viewer", "expires_in_days": 0 }),
        json!({ "role": "viewer", "expires_in_days": 31 }),
        json!({ "role": "viewer", "max_uses": 0 }),
    ] {
        assert_eq!(
            app.post(&uri, body.clone(), Some(&orga)).await.status,
            StatusCode::BAD_REQUEST,
            "{body}"
        );
    }
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn existing_member_keeps_higher_role_and_does_not_consume(db: PgPool) {
    let app = TestApp::new(db.clone());
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let bob = app.register("bob@example.com", "Bob").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &bob, "editor").await;

    let token = app.invite(&orga, &id, "viewer").await;
    let res = app
        .post(
            &format!("/api/v1/invitations/{token}/accept"),
            json!({}),
            Some(&bob),
        )
        .await;
    assert_eq!(res.body["role"], "editor");
    // L'organisateur qui ouvre son propre lien reste organisateur.
    let res = app
        .post(
            &format!("/api/v1/invitations/{token}/accept"),
            json!({}),
            Some(&orga),
        )
        .await;
    assert_eq!(res.body["role"], "organizer");
    let uses: i32 =
        sqlx::query_scalar("SELECT uses FROM invitations ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(uses, 0);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn magic_link_reclaims_account_created_by_someone_else(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let id = app.create_trip(&orga, "Voyage").await;
    let token = app.invite(&orga, &id, "viewer").await;
    // Quelqu'un accepte l'invitation avec l'email d'une autre personne.
    let res = app
        .post(
            &format!("/api/v1/invitations/{token}/accept"),
            json!({ "display_name": "Usurpateur", "email": "victime@example.com" }),
            None,
        )
        .await;
    let squatter = session_token(&res).unwrap();

    // La vraie propriétaire se connecte par lien magique (preuve de possession de l'email).
    app.post(
        "/api/v1/auth/magic-link",
        json!({ "email": "victime@example.com", "next": format!("/invite/{token}") }),
        None,
    )
    .await;
    let mails = app.wait_for_mail(1).await;
    assert!(mails[0].text.contains(&format!("&next=/invite/{token}")));
    let magic = mails[0]
        .text
        .split("token=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    let res = app
        .post(
            "/api/v1/auth/magic-link/verify",
            json!({ "token": magic }),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);

    // La session de l'usurpateur est fermée.
    assert_eq!(
        app.get("/api/v1/me", Some(&squatter)).await.status,
        StatusCode::UNAUTHORIZED
    );
}
