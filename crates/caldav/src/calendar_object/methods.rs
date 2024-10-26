use crate::Error;
use actix_web::http::header;
use actix_web::http::header::HeaderValue;
use actix_web::web::{Data, Path};
use actix_web::HttpRequest;
use actix_web::HttpResponse;
use rustical_store::auth::User;
use rustical_store::model::CalendarObject;
use rustical_store::CalendarStore;
use tokio::sync::RwLock;
use tracing::instrument;
use tracing_actix_web::RootSpan;

use super::resource::CalendarObjectPathComponents;

#[instrument(parent = root_span.id(), skip(store, root_span))]
pub async fn get_event<C: CalendarStore + ?Sized>(
    path: Path<CalendarObjectPathComponents>,
    store: Data<RwLock<C>>,
    user: User,
    root_span: RootSpan,
) -> Result<HttpResponse, Error> {
    let CalendarObjectPathComponents {
        principal,
        cal_id,
        object_id,
    } = path.into_inner();

    if user.id != principal {
        return Ok(HttpResponse::Unauthorized().body(""));
    }

    let calendar = store.read().await.get_calendar(&principal, &cal_id).await?;
    if user.id != calendar.principal {
        return Ok(HttpResponse::Unauthorized().body(""));
    }

    let event = store
        .read()
        .await
        .get_object(&principal, &cal_id, &object_id)
        .await?;

    Ok(HttpResponse::Ok()
        .insert_header(("ETag", event.get_etag()))
        .insert_header(("Content-Type", "text/calendar"))
        .body(event.get_ics().to_owned()))
}

#[instrument(parent = root_span.id(), skip(store, req, root_span))]
pub async fn put_event<C: CalendarStore + ?Sized>(
    path: Path<CalendarObjectPathComponents>,
    store: Data<RwLock<C>>,
    body: String,
    user: User,
    req: HttpRequest,
    root_span: RootSpan,
) -> Result<HttpResponse, Error> {
    let CalendarObjectPathComponents {
        principal,
        cal_id,
        object_id,
    } = path.into_inner();

    if user.id != principal {
        return Ok(HttpResponse::Unauthorized().body(""));
    }

    let calendar = store.read().await.get_calendar(&principal, &cal_id).await?;
    if user.id != calendar.principal {
        return Ok(HttpResponse::Unauthorized().body(""));
    }

    // TODO: implement If-Match
    //
    let mut store_write = store.write().await;

    if Some(&HeaderValue::from_static("*")) == req.headers().get(header::IF_NONE_MATCH) {
        // Only write if not existing
        match store_write
            .get_object(&principal, &cal_id, &object_id)
            .await
        {
            Ok(_) => {
                // Conflict
                return Ok(HttpResponse::Conflict().body("Resource with this URI already exists"));
            }
            Err(rustical_store::Error::NotFound) => {
                // Path unused, we can proceed
            }
            Err(err) => {
                // Some unknown error :(
                return Err(err.into());
            }
        }
    }

    let object = CalendarObject::from_ics(object_id, body)?;
    store_write.put_object(principal, cal_id, object).await?;

    Ok(HttpResponse::Created().body(""))
}
