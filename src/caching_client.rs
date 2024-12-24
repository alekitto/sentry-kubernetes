use crate::k8s::{ApiResource, DynamicObject};
use k8s_openapi::api::core::v1::{Node, ObjectReference, Pod};
use kube::{Api, Client};
use mini_moka::sync::Cache;
use std::time::Duration;

pub struct CachingClient {
    client: Client,
    pod_api: Api<Pod>,
    node_api: Api<Node>,

    pod_cache: Cache<String, Pod>,
    node_cache: Cache<String, Node>,
    obj_cache: Cache<String, DynamicObject>,
}

impl CachingClient {
    pub fn new(client: Client) -> Self {
        Self {
            client: client.clone(),
            pod_api: Api::all(client.clone()),
            node_api: Api::all(client),
            pod_cache: Cache::builder()
                .time_to_live(Duration::from_secs(5))
                .build(),
            node_cache: Cache::builder()
                .time_to_live(Duration::from_secs(5))
                .build(),
            obj_cache: Cache::builder()
                .time_to_live(Duration::from_secs(5))
                .build(),
        }
    }

    pub fn inner(&self) -> Client {
        self.client.clone()
    }

    pub async fn get_pod(&self, name: &String) -> Option<Pod> {
        if let Some(p) = self.pod_cache.get(name) {
            Some(p)
        } else {
            let pod = self.pod_api.get(name).await.ok()?;
            self.pod_cache.insert(name.clone(), pod.clone());

            Some(pod)
        }
    }

    pub async fn get_node(&self, name: &String) -> Option<Node> {
        if let Some(n) = self.node_cache.get(name) {
            Some(n)
        } else {
            let node = self.node_api.get(name).await.ok()?;
            self.node_cache.insert(name.clone(), node.clone());

            Some(node)
        }
    }

    pub async fn get_obj(&self, obj: &ObjectReference) -> Option<DynamicObject> {
        let name = obj.name.clone()?;
        let api_version = obj.api_version.clone().unwrap_or_else(|| "v1".to_string());
        let kind = obj.kind.clone().unwrap_or_default();

        let cache_key = format!("{}_{}_{}", &api_version, &kind, &name);
        if let Some(n) = self.obj_cache.get(&cache_key) {
            Some(n)
        } else {
            let (group, version) = if api_version.contains('/') {
                let split: Vec<&str> = api_version.splitn(2, '/').collect();
                (split[0].to_string(), split[1].to_string())
            } else {
                ("".to_string(), api_version.clone())
            };

            let plural = format!("{}s", &kind);
            let api_resource = ApiResource {
                group,
                version,
                api_version,
                kind,
                plural,
            };

            let api = if let Some(ns) = obj.namespace.as_deref() {
                Api::<DynamicObject>::namespaced_with(self.inner(), ns, &api_resource)
            } else {
                Api::<DynamicObject>::all_with(self.inner(), &api_resource)
            };

            let obj = api.get(&name).await.ok()?;
            self.obj_cache.insert(cache_key.clone(), obj.clone());

            Some(obj)
        }
    }
}
