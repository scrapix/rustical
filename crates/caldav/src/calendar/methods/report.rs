use actix_web::{
    http::header::ContentType,
    web::{Data, Path},
    HttpRequest, HttpResponse,
};
use rustical_auth::{AuthInfoExtractor, CheckAuthentication};
use rustical_dav::{
    namespace::Namespace,
    propfind::{MultistatusElement, PropElement, PropfindType, ServicePrefix},
    resource::HandlePropfind,
};
use rustical_store::{calendar::CalendarStore, event::Event};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{event::resource::EventFile, Error};

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum PropQuery {
    Allprop,
    Prop,
    Propname,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
// <!ELEMENT calendar-query ((DAV:allprop | DAV:propname | DAV:prop)?, href+)>
pub struct CalendarMultigetRequest {
    #[serde(flatten)]
    prop: PropfindType,
    href: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
struct TimeRangeElement {
    #[serde(rename = "@start")]
    start: Option<String>,
    #[serde(rename = "@end")]
    end: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
struct ParamFilterElement {
    is_not_defined: Option<()>,
    text_match: Option<TextMatchElement>,

    #[serde(rename = "@name")]
    name: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
struct TextMatchElement {
    #[serde(rename = "@collation")]
    collation: String,
    #[serde(rename = "@negate-collation")]
    negate_collation: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
struct PropFilterElement {
    is_not_defined: Option<()>,
    time_range: Option<TimeRangeElement>,
    text_match: Option<TextMatchElement>,
    #[serde(default)]
    param_filter: Vec<ParamFilterElement>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
struct CompFilterElement {
    is_not_defined: Option<()>,
    time_range: Option<TimeRangeElement>,
    #[serde(default)]
    prop_filter: Vec<PropFilterElement>,
    #[serde(default)]
    comp_filter: Vec<CompFilterElement>,

    #[serde(rename = "@name")]
    name: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
struct FilterElement {
    comp_filter: CompFilterElement,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
// #[serde(rename = "calendar-query")]
// <!ELEMENT calendar-query ((DAV:allprop | DAV:propname | DAV:prop)?, filter, timezone?)>
pub struct CalendarQueryRequest {
    #[serde(flatten)]
    prop: PropfindType,
    filter: Option<FilterElement>,
    timezone: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum ReportRequest {
    CalendarMultiget(CalendarMultigetRequest),
    CalendarQuery(CalendarQueryRequest),
}

async fn get_events_calendar_query<C: CalendarStore + ?Sized>(
    cal_query: CalendarQueryRequest,
    cid: &str,
    store: &RwLock<C>,
) -> Result<Vec<Event>, Error> {
    // TODO: Implement filtering
    Ok(store.read().await.get_events(cid).await?)
}

async fn get_events_calendar_multiget<C: CalendarStore + ?Sized>(
    cal_query: CalendarMultigetRequest,
    cid: &str,
    store: &RwLock<C>,
) -> Result<Vec<Event>, Error> {
    let mut events = Vec::new();
    for href in cal_query.href {
        dbg!(href);
        // let uid =
        // events.push(store.read().await.get_event(cid, &uid))
    }
    Ok(events)
}

pub async fn route_report_calendar<A: CheckAuthentication, C: CalendarStore + ?Sized>(
    path: Path<(String, String)>,
    body: String,
    auth: AuthInfoExtractor<A>,
    req: HttpRequest,
    cal_store: Data<RwLock<C>>,
    prefix: Data<ServicePrefix>,
) -> Result<HttpResponse, Error> {
    let (principal, cid) = path.into_inner();
    if principal != auth.inner.user_id {
        return Err(Error::Unauthorized);
    }

    let request: ReportRequest = quick_xml::de::from_str(&body).map_err(|err| {
        dbg!(err.to_string());
        Error::InternalError
    })?;
    let events = match request.clone() {
        ReportRequest::CalendarQuery(cal_query) => {
            get_events_calendar_query(cal_query, &cid, &cal_store).await?
        }
        ReportRequest::CalendarMultiget(cal_multiget) => {
            get_events_calendar_multiget(cal_multiget, &cid, &cal_store).await?
        }
    };

    // TODO: Change this
    let proptag = match request {
        ReportRequest::CalendarQuery(CalendarQueryRequest { prop, .. }) => prop.clone(),
        ReportRequest::CalendarMultiget(CalendarMultigetRequest { prop, .. }) => prop.clone(),
    };
    let props = match proptag {
        PropfindType::Allprop => {
            vec!["allprop".to_owned()]
        }
        PropfindType::Propname => {
            // TODO: Implement
            return Err(Error::InternalError);
        }
        PropfindType::Prop(PropElement { prop: prop_tags }) => prop_tags.into(),
    };
    let props: Vec<&str> = props.iter().map(String::as_str).collect();

    let mut responses = Vec::new();
    for event in events {
        responses.push(
            EventFile {
                path: format!("{}/{}", req.path(), event.get_uid()),
                event,
            }
            .propfind(&prefix.0, props.clone())
            .await?,
        );
    }

    let mut output = String::new();
    let mut ser = quick_xml::se::Serializer::new(&mut output);
    ser.indent(' ', 4);
    MultistatusElement {
        responses,
        member_responses: Vec::<String>::new(),
        ns_dav: Namespace::Dav.as_str(),
        ns_caldav: Namespace::CalDAV.as_str(),
        ns_ical: Namespace::ICal.as_str(),
    }
    .serialize(ser)
    .unwrap();

    Ok(HttpResponse::MultiStatus()
        .content_type(ContentType::xml())
        .body(output))
}
