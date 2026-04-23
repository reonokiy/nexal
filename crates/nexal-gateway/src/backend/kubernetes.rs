//! Kubernetes backend — creates Pods via the `kube` client.
//!
//! Design assumptions:
//! - Gateway runs **inside** the cluster (uses in-cluster config or
//!   an explicit kubeconfig).
//! - Agent WS connectivity is via **Pod IP:9100** — no Service needed.
//! - The `nexal-agent` binary is distributed via an **initContainer**
//!   that copies it from a small image to an emptyDir volume. The main
//!   container then runs the agent from that shared volume.
//! - Containers are never recycled — torn down and recreated from
//!   scratch every time.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EmptyDirVolumeSource, EnvVar, Pod, PodSpec, ResourceRequirements,
    SecurityContext, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::{Api, DeleteParams, PostParams};
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{Client, Config};
use tokio::time::sleep;
use tracing::warn;

use super::{BackendError, ContainerBackend, ContainerHandle, ContainerSpec};

const AGENT_WS_PORT: u16 = 9100;
const AGENT_VOLUME_NAME: &str = "nexal-agent-bin";
const AGENT_MOUNT_PATH: &str = "/opt/nexal/bin";
const AGENT_BIN_PATH: &str = "/opt/nexal/bin/nexal-agent";

pub struct KubernetesBackend {
    client: Client,
    namespace: String,
    /// Image that contains `/usr/local/bin/nexal-agent` — used as the
    /// initContainer source. This should be a small, purpose-built
    /// image that only ships the static binary.
    agent_init_image: String,
}

impl KubernetesBackend {
    pub async fn new(cfg: KubernetesConfig) -> Result<Self, BackendError> {
        let client = match cfg.kubeconfig {
            Some(path) => {
                let kubeconfig = Kubeconfig::read_from(&path)
                    .map_err(|e| BackendError::Io(format!("read kubeconfig {}: {e}", path.display())))?;
                let config = Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default())
                    .await
                    .map_err(|e| BackendError::Io(format!("build kube config: {e}")))?;
                Client::try_from(config)
                    .map_err(|e| BackendError::Io(format!("create kube client: {e}")))?
            }
            None => {
                // In-cluster config (ServiceAccount token mount).
                Client::try_default()
                    .await
                    .map_err(|e| BackendError::Io(format!("in-cluster kube config: {e}")))?
            }
        };
        Ok(Self {
            client,
            namespace: cfg.namespace.unwrap_or_else(|| "default".to_string()),
            agent_init_image: cfg.agent_init_image,
        })
    }

    fn pods(&self) -> Api<Pod> {
        Api::namespaced(self.client.clone(), &self.namespace)
    }

    fn build_pod(&self, spec: &ContainerSpec) -> Pod {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "nexal".to_string());
        labels.insert("nexal.kind".to_string(), "worker".to_string());
        labels.insert("nexal.session".to_string(), spec.name.clone());
        for (k, v) in &spec.labels {
            labels.insert(k.clone(), v.clone());
        }

        let env: Vec<EnvVar> = spec
            .env
            .iter()
            .map(|(k, v)| EnvVar {
                name: k.clone(),
                value: Some(v.clone()),
                ..Default::default()
            })
            .collect();

        // Resource limits.
        let mut limits = BTreeMap::new();
        if let Some(mem) = &spec.memory {
            limits.insert("memory".to_string(), Quantity(mem.clone()));
        }
        if let Some(cpus) = &spec.cpus {
            limits.insert("cpu".to_string(), Quantity(cpus.clone()));
        }

        // Shared emptyDir for the agent binary.
        let agent_volume = Volume {
            name: AGENT_VOLUME_NAME.to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        };
        let agent_volume_mount = VolumeMount {
            name: AGENT_VOLUME_NAME.to_string(),
            mount_path: AGENT_MOUNT_PATH.to_string(),
            ..Default::default()
        };

        // initContainer: copy nexal-agent from init image to shared volume.
        let init_container = Container {
            name: "copy-agent".to_string(),
            image: Some(self.agent_init_image.clone()),
            command: Some(vec!["cp".into(), "/usr/local/bin/nexal-agent".into(), AGENT_BIN_PATH.into()]),
            volume_mounts: Some(vec![agent_volume_mount.clone()]),
            ..Default::default()
        };

        // Main container.
        let mut volumes = vec![agent_volume];
        let mut volume_mounts = vec![agent_volume_mount];

        // Workspace volume — in K8s this is an emptyDir unless we add PVC support later.
        if spec.workspace_volume.is_some() {
            volumes.push(Volume {
                name: "workspace".to_string(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
                ..Default::default()
            });
            volume_mounts.push(VolumeMount {
                name: "workspace".to_string(),
                mount_path: "/workspace".to_string(),
                ..Default::default()
            });
        }

        // Proxy socket directory.
        volumes.push(Volume {
            name: "proxy-sockets".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        });
        volume_mounts.push(VolumeMount {
            name: "proxy-sockets".to_string(),
            mount_path: "/run/nexal/proxy".to_string(),
            ..Default::default()
        });

        let main_container = Container {
            name: "agent".to_string(),
            image: Some(spec.image.clone()),
            command: Some(vec![
                AGENT_BIN_PATH.to_string(),
                "--listen".to_string(),
                format!("ws://0.0.0.0:{AGENT_WS_PORT}"),
            ]),
            ports: Some(vec![ContainerPort {
                container_port: AGENT_WS_PORT as i32,
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            env: Some(env),
            volume_mounts: Some(volume_mounts),
            resources: Some(ResourceRequirements {
                limits: if limits.is_empty() { None } else { Some(limits) },
                ..Default::default()
            }),
            working_dir: Some("/workspace".to_string()),
            security_context: Some(SecurityContext {
                allow_privilege_escalation: Some(false),
                run_as_non_root: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };

        Pod {
            metadata: kube::api::ObjectMeta {
                name: Some(spec.name.clone()),
                namespace: Some(self.namespace.clone()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(PodSpec {
                init_containers: Some(vec![init_container]),
                containers: vec![main_container],
                volumes: Some(volumes),
                restart_policy: Some("Never".to_string()),
                // Don't inject service account token — agent doesn't need K8s access.
                automount_service_account_token: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Wait for a Pod to reach the Running phase and have a Pod IP.
    /// Returns the Pod IP.
    async fn wait_for_pod_ip(&self, name: &str) -> Result<String, BackendError> {
        let pods = self.pods();
        for _ in 0..60u32 {
            match pods.get(name).await {
                Ok(pod) => {
                    if let Some(status) = &pod.status {
                        let phase = status.phase.as_deref().unwrap_or("");
                        match phase {
                            "Running" => {
                                if let Some(ip) = &status.pod_ip {
                                    if !ip.is_empty() {
                                        return Ok(ip.clone());
                                    }
                                }
                            }
                            "Failed" | "Succeeded" => {
                                return Err(BackendError::Cli(format!(
                                    "pod {name} in terminal phase: {phase}"
                                )));
                            }
                            _ => {} // Pending — keep waiting.
                        }
                    }
                }
                Err(kube::Error::Api(resp)) if resp.code == 404 => {
                    return Err(BackendError::Cli(format!("pod {name} not found")));
                }
                Err(e) => {
                    warn!("get pod {name}: {e}");
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
        Err(BackendError::PortDiscovery(format!(
            "pod {name}: timed out waiting for Running phase + pod IP"
        )))
    }
}

#[async_trait]
impl ContainerBackend for KubernetesBackend {
    fn name(&self) -> &'static str {
        "kubernetes"
    }

    async fn ensure(&self, spec: ContainerSpec) -> Result<ContainerHandle, BackendError> {
        let pods = self.pods();

        // Check if pod already exists.
        match pods.get(&spec.name).await {
            Ok(pod) => {
                let phase = pod
                    .status
                    .as_ref()
                    .and_then(|s| s.phase.as_deref())
                    .unwrap_or("");
                match phase {
                    "Running" => {
                        // Already running — discover IP and return.
                        let ip = pod
                            .status
                            .as_ref()
                            .and_then(|s| s.pod_ip.as_ref())
                            .ok_or_else(|| {
                                BackendError::PortDiscovery(format!(
                                    "pod {} running but no IP",
                                    spec.name
                                ))
                            })?;
                        return Ok(ContainerHandle {
                            name: spec.name,
                            ws_url: format!("ws://{ip}:{AGENT_WS_PORT}"),
                        });
                    }
                    _ => {
                        // Non-running state — delete and recreate.
                        let _ = pods
                            .delete(&spec.name, &DeleteParams::default().grace_period(0))
                            .await;
                        // Wait for deletion to complete.
                        for _ in 0..30u32 {
                            match pods.get(&spec.name).await {
                                Err(kube::Error::Api(resp)) if resp.code == 404 => break,
                                _ => sleep(Duration::from_millis(500)).await,
                            }
                        }
                    }
                }
            }
            Err(kube::Error::Api(resp)) if resp.code == 404 => {
                // Doesn't exist — will create below.
            }
            Err(e) => {
                return Err(BackendError::Io(format!("get pod {}: {e}", spec.name)));
            }
        }

        // Create the pod.
        let pod = self.build_pod(&spec);
        pods.create(&PostParams::default(), &pod)
            .await
            .map_err(|e| BackendError::Cli(format!("create pod {}: {e}", spec.name)))?;

        let ip = self.wait_for_pod_ip(&spec.name).await?;
        Ok(ContainerHandle {
            name: spec.name,
            ws_url: format!("ws://{ip}:{AGENT_WS_PORT}"),
        })
    }

    async fn destroy(&self, name: &str) -> Result<(), BackendError> {
        let pods = self.pods();
        match pods.delete(name, &DeleteParams::default().grace_period(0)).await {
            Ok(_) => Ok(()),
            // Already gone — idempotent.
            Err(kube::Error::Api(resp)) if resp.code == 404 => Ok(()),
            Err(e) => Err(BackendError::Cli(format!("delete pod {name}: {e}"))),
        }
    }

    async fn exists(&self, name: &str) -> Result<bool, BackendError> {
        match self.pods().get(name).await {
            Ok(_) => Ok(true),
            Err(kube::Error::Api(resp)) if resp.code == 404 => Ok(false),
            Err(e) => Err(BackendError::Io(format!("get pod {name}: {e}"))),
        }
    }

    async fn ws_url(&self, name: &str) -> Result<String, BackendError> {
        let ip = self.wait_for_pod_ip(name).await?;
        Ok(format!("ws://{ip}:{AGENT_WS_PORT}"))
    }
}

/// Configuration for the Kubernetes backend, deserialized from
/// `[backend]` in `gateway.toml`.
#[derive(Debug, Clone)]
pub struct KubernetesConfig {
    /// Namespace for worker pods. Defaults to `"default"`.
    pub namespace: Option<String>,
    /// Path to kubeconfig file. `None` = in-cluster config.
    pub kubeconfig: Option<std::path::PathBuf>,
    /// Image containing `/usr/local/bin/nexal-agent`. Used as
    /// initContainer to copy the binary into a shared emptyDir.
    pub agent_init_image: String,
}
