use std::collections::HashSet;
use std::sync::Mutex;
use anyhow::Result;

/// Tracks which GPUs exist and which are currently busy.
/// Held as Arc<GpuPool> in shared server state, same as the database.
pub struct GpuPool {
    /// All GPUs indexed by id
    gpus: Vec<GpuConfig>,
    /// IDs of GPUs currently assigned to an active task
    busy: Mutex<HashSet<String>>,
}

impl GpuPool {
    /// Build the pool from hardware.json at startup.
    pub fn from_config(config: HardwareConfig) -> Self {
        println!("GPU pool initialized:");
        for gpu in &config.gpus {
            println!(
                "  [{:?}] {} — {} ({})",
                gpu.role, gpu.id, gpu.model, gpu.url
            );
        }

        Self {
            gpus: config.gpus,
            busy: Mutex::new(HashSet::new()),
        }
    }

    /// Load hardware.json and build the pool in one step.
    /// Called once at engine startup.
    pub fn load() -> Result<Self> {
        let config = HardwareConfig::load()?;
        Ok(Self::from_config(config))
    }

    /// Acquire a free interactive GPU.
    /// Returns None if all interactive GPUs are currently busy.
    /// The caller is responsible for releasing the GPU when the task completes.
    pub fn acquire_interactive(&self) -> Option<GpuHandle> {
        self.acquire(GpuRole::Interactive)
    }

    /// Acquire a free background GPU.
    /// Returns None if all background GPUs are currently busy.
    pub fn acquire_background(&self) -> Option<GpuHandle> {
        self.acquire(GpuRole::Background)
    }

    /// Release a GPU back to the pool.
    /// Should be called when a task completes, errors, or is abandoned.
    pub fn release(&self, gpu_id: &str) {
        let mut busy = self.busy.lock().unwrap();
        let removed = busy.remove(gpu_id);
        if removed {
            println!("GPU released: {}", gpu_id);
        } else {
            eprintln!("Warning: tried to release GPU '{}' that wasn't marked busy", gpu_id);
        }
    }

    /// How many interactive GPUs are currently free.
    pub fn interactive_available(&self) -> usize {
        let busy = self.busy.lock().unwrap();
        self.gpus.iter()
            .filter(|g| g.role == GpuRole::Interactive && !busy.contains(&g.id))
            .count()
    }

    /// How many background GPUs are currently free.
    pub fn background_available(&self) -> usize {
        let busy = self.busy.lock().unwrap();
        self.gpus.iter()
            .filter(|g| g.role == GpuRole::Background && !busy.contains(&g.id))
            .count()
    }

    /// All GPUs and their current status. Useful for a status endpoint.
    pub fn status(&self) -> Vec<GpuStatus> {
        let busy = self.busy.lock().unwrap();
        self.gpus.iter()
            .map(|g| GpuStatus {
                id: g.id.clone(),
                url: g.url.clone(),
                model: g.model.clone(),
                role: g.role.clone(),
                description: g.description.clone(),
                busy: busy.contains(&g.id),
            })
            .collect()
    }

    fn acquire(&self, role: GpuRole) -> Option<GpuHandle> {
        let mut busy = self.busy.lock().unwrap();

        let gpu = self.gpus.iter()
            .find(|g| g.role == role && !busy.contains(&g.id))?;

        busy.insert(gpu.id.clone());
        println!("GPU acquired: {} for {:?} task", gpu.id, role);

        Some(GpuHandle::from_config(gpu))
    }
}

/// Public status view of a single GPU — used for the status endpoint.
#[derive(Debug, serde::Serialize)]
pub struct GpuStatus {
    pub id: String,
    pub url: String,
    pub model: String,
    pub role: GpuRole,
    pub description: String,
    pub busy: bool,
}

use serde::{Deserialize, Serialize};

/// The role a GPU plays in the system.
/// Interactive GPUs run the Orchestrator for user-facing tasks.
/// Background GPUs run the worker for summarization, memory extraction, etc.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GpuRole {
    Interactive,
    Background,
}

/// A single GPU entry as defined in hardware.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    pub id: String,
    pub url: String,
    pub model: String,
    pub role: GpuRole,
    #[serde(default)]
    pub description: String,
}

/// The full hardware.json structure
#[derive(Debug, Deserialize)]
pub struct HardwareConfig {
    pub gpus: Vec<GpuConfig>,
}

impl HardwareConfig {
    /// Load and parse hardware.json from the workspace root.
    /// Panics at startup if the file is missing or malformed —
    /// we want to fail fast rather than discover this at request time.
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::find_config()?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!(
                "Failed to read hardware.json at {}: {}",
                path.display(), e
            ))?;

        serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!(
                "Failed to parse hardware.json: {}", e
            ))
    }

    /// Walk up from current directory to find hardware.json.
    /// Handles running from workspace root or from a crate subdirectory.
    fn find_config() -> anyhow::Result<std::path::PathBuf> {
        let mut dir = std::env::current_dir()?;

        loop {
            let candidate = dir.join("hardware.json");
            if candidate.exists() {
                return Ok(candidate);
            }

            if !dir.pop() {
                return Err(anyhow::anyhow!(
                    "hardware.json not found. Create it in the workspace root."
                ));
            }
        }
    }
}

/// A live GPU handle acquired from the pool.
/// Holds everything the Orchestrator or worker needs to make calls.
/// When dropped, the GPU is NOT automatically released — call pool.release() explicitly
/// so release can be logged and the state transition is intentional.
#[derive(Debug, Clone)]
pub struct GpuHandle {
    pub id: String,
    pub url: String,
    pub model: String,
    pub role: GpuRole,
}

impl GpuHandle {
    pub fn from_config(config: &GpuConfig) -> Self {
        Self {
            id: config.id.clone(),
            url: config.url.clone(),
            model: config.model.clone(),
            role: config.role.clone(),
        }
    }
}