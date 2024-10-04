use crate::Error;
use actix_web::{dev::ResourceMap, web::Data, HttpRequest};
use async_trait::async_trait;
use derive_more::derive::{From, Into};
use rustical_dav::resource::{InvalidProperty, Resource, ResourceService};
use rustical_store::model::object::CalendarObject;
use rustical_store::CalendarStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strum::{EnumString, VariantNames};
use tokio::sync::RwLock;

use super::methods::{get_event, put_event};

pub struct CalendarObjectResourceService<C: CalendarStore + ?Sized> {
    pub cal_store: Arc<RwLock<C>>,
    pub path: String,
    pub principal: String,
    pub cid: String,
    pub uid: String,
}

#[derive(EnumString, Debug, VariantNames, Clone)]
#[strum(serialize_all = "kebab-case")]
pub enum CalendarObjectPropName {
    Getetag,
    CalendarData,
    Getcontenttype,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarObjectProp {
    Getetag(String),
    #[serde(rename = "C:calendar-data")]
    CalendarData(String),
    Getcontenttype(String),
    #[serde(other)]
    Invalid,
}

impl InvalidProperty for CalendarObjectProp {
    fn invalid_property(&self) -> bool {
        matches!(self, Self::Invalid)
    }
}

#[derive(Clone, From, Into)]
pub struct CalendarObjectResource(CalendarObject);

impl Resource for CalendarObjectResource {
    type PropName = CalendarObjectPropName;
    type Prop = CalendarObjectProp;
    type Error = Error;

    fn get_prop(
        &self,
        _rmap: &ResourceMap,
        prop: Self::PropName,
    ) -> Result<Self::Prop, Self::Error> {
        Ok(match prop {
            CalendarObjectPropName::Getetag => CalendarObjectProp::Getetag(self.0.get_etag()),
            CalendarObjectPropName::CalendarData => {
                CalendarObjectProp::CalendarData(self.0.get_ics().to_owned())
            }
            CalendarObjectPropName::Getcontenttype => {
                CalendarObjectProp::Getcontenttype("text/calendar;charset=utf-8".to_owned())
            }
        })
    }

    #[inline]
    fn resource_name() -> &'static str {
        "caldav_calendar_object"
    }
}

#[async_trait(?Send)]
impl<C: CalendarStore + ?Sized> ResourceService for CalendarObjectResourceService<C> {
    type PathComponents = (String, String, String); // principal, calendar, event
    type Resource = CalendarObjectResource;
    type MemberType = CalendarObjectResource;
    type Error = Error;

    async fn new(
        req: &HttpRequest,
        path_components: Self::PathComponents,
    ) -> Result<Self, Self::Error> {
        let (principal, cid, mut uid) = path_components;

        if uid.ends_with(".ics") {
            uid.truncate(uid.len() - 4);
        }

        let cal_store = req
            .app_data::<Data<RwLock<C>>>()
            .expect("no calendar store in app_data!")
            .clone()
            .into_inner();

        Ok(Self {
            cal_store,
            principal,
            cid,
            uid,
            path: req.path().to_string(),
        })
    }

    async fn get_resource(&self, principal: String) -> Result<Self::Resource, Self::Error> {
        if self.principal != principal {
            return Err(Error::Unauthorized);
        }
        let event = self
            .cal_store
            .read()
            .await
            .get_object(&self.principal, &self.cid, &self.uid)
            .await?;
        Ok(event.into())
    }

    async fn save_resource(&self, _file: Self::Resource) -> Result<(), Self::Error> {
        Err(Error::NotImplemented)
    }

    async fn delete_resource(&self, use_trashbin: bool) -> Result<(), Self::Error> {
        self.cal_store
            .write()
            .await
            .delete_object(&self.principal, &self.cid, &self.uid, use_trashbin)
            .await?;
        Ok(())
    }

    #[inline]
    fn actix_additional_routes(res: actix_web::Resource) -> actix_web::Resource {
        res.get(get_event::<C>).put(put_event::<C>)
    }
}
