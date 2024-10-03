use actix_web::http::Method;
use actix_web::web::{self, Data};
use actix_web::{guard, HttpResponse, Responder};
use calendar::resource::CalendarResourceService;
use calendar_object::resource::CalendarObjectResourceService;
use principal::PrincipalResourceService;
use root::RootResourceService;
use rustical_dav::methods::{
    propfind::ServicePrefix, route_delete, route_propfind, route_proppatch,
};
use rustical_store::auth::{AuthenticationMiddleware, AuthenticationProvider};
use rustical_store::CalendarStore;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod calendar;
pub mod calendar_object;
pub mod error;
pub mod principal;
pub mod root;

pub use error::Error;

pub struct CalDavContext<C: CalendarStore + ?Sized> {
    pub store: Arc<RwLock<C>>,
}

pub fn configure_well_known(cfg: &mut web::ServiceConfig, caldav_root: String) {
    cfg.service(web::redirect("/caldav", caldav_root).permanent());
}

pub fn configure_dav<AP: AuthenticationProvider, C: CalendarStore + ?Sized>(
    cfg: &mut web::ServiceConfig,
    prefix: String,
    auth_provider: Arc<AP>,
    store: Arc<RwLock<C>>,
) {
    let propfind_method = || web::method(Method::from_str("PROPFIND").unwrap());
    let proppatch_method = || web::method(Method::from_str("PROPPATCH").unwrap());
    let report_method = || web::method(Method::from_str("REPORT").unwrap());
    let mkcalendar_method = || web::method(Method::from_str("MKCALENDAR").unwrap());

    cfg.service(
        web::scope("")
            .wrap(AuthenticationMiddleware::new(auth_provider))
            .app_data(Data::new(CalDavContext {
                store: store.clone(),
            }))
            .app_data(Data::new(ServicePrefix(prefix)))
            .app_data(Data::from(store.clone()))
            .service(
                web::resource("{path:.*}")
                    // Without the guard this service would handle all requests
                    .guard(guard::Method(Method::OPTIONS))
                    .to(options_handler),
            )
            .service(
                web::resource("")
                    .route(propfind_method().to(route_propfind::<RootResourceService>))
                    .route(proppatch_method().to(route_proppatch::<RootResourceService>)),
            )
            .service(
                web::scope("/user").service(
                    web::scope("/{principal}")
                        .service(
                            web::resource("")
                                .route(
                                    propfind_method()
                                        .to(route_propfind::<PrincipalResourceService<C>>),
                                )
                                .route(
                                    proppatch_method()
                                        .to(route_proppatch::<PrincipalResourceService<C>>),
                                ),
                        )
                        .service(
                            web::scope("/{calendar}")
                                .service(
                                    web::resource("")
                                        .route(report_method().to(
                                            calendar::methods::report::route_report_calendar::<C>,
                                        ))
                                        .route(
                                            propfind_method()
                                                .to(route_propfind::<CalendarResourceService<C>>),
                                        )
                                        .route(
                                            proppatch_method()
                                                .to(route_proppatch::<CalendarResourceService<C>>),
                                        )
                                        .route(
                                            web::method(Method::DELETE)
                                                .to(route_delete::<CalendarResourceService<C>>),
                                        )
                                        .route(mkcalendar_method().to(
                                            calendar::methods::mkcalendar::route_mkcalendar::<C>,
                                        )),
                                )
                                .service(
                                    web::resource("/{event}")
                                        .route(
                                            propfind_method().to(route_propfind::<
                                                CalendarObjectResourceService<C>,
                                            >),
                                        )
                                        .route(proppatch_method().to(route_proppatch::<
                                            CalendarObjectResourceService<C>,
                                        >))
                                        .route(
                                            web::method(Method::DELETE).to(route_delete::<
                                                CalendarObjectResourceService<C>,
                                            >),
                                        )
                                        .route(
                                            web::method(Method::GET)
                                                .to(calendar_object::methods::get_event::<C>),
                                        )
                                        .route(
                                            web::method(Method::PUT)
                                                .to(calendar_object::methods::put_event::<C>),
                                        ),
                                ),
                        ),
                ),
            ),
    );
}

async fn options_handler() -> impl Responder {
    HttpResponse::Ok()
        .insert_header((
            "Allow",
            "OPTIONS, GET, HEAD, POST, PUT, REPORT, PROPFIND, PROPPATCH, MKCALENDAR",
        ))
        .insert_header((
            "DAV",
            "1, 2, 3, calendar-access, extended-mkcol",
            // "1, 2, 3, calendar-access, addressbook, extended-mkcol",
        ))
        .body("options")
}
