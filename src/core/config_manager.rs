use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::fs;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use anyhow::Result;
use crate::config::Config;
use crate::core::lock::lock_mutex;

pub struct ConfigManager {
    config: Arc<Mutex<Config>>,
    last_modified: Arc<Mutex<SystemTime>>,
    config_path: String,
}

impl ConfigManager {
    pub fn new(path: &str) -> Result<Self> {
        let config = Config::load(path)?;
        let metadata = fs::metadata(path)?;
        let modified = metadata.modified()?;
        
        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            last_modified: Arc::new(Mutex::new(modified)),
            config_path: path.to_string(),
        })
    }
    
    pub fn get_config(&self) -> Config {
        let config = lock_mutex(&self.config, "config_manager.get_config");
        config.clone()
    }
    
    pub fn start_watcher(&self) -> Result<()> {
        let config_clone = self.config.clone();
        let modified_clone = self.last_modified.clone();
        let path_clone = self.config_path.clone();
        
        std::thread::spawn(move || {
            let mut watcher: RecommendedWatcher = Watcher::new_immediate(move |res| {
                match res {
                    Ok(event) => {
                        if event.paths.iter().any(|p| p.to_string_lossy().contains(&path_clone)) {
                            match Self::try_reload_config(&path_clone, &config_clone, &modified_clone) {
                                Ok(true) => log::info!("Config reloaded successfully"),
                                Ok(false) => log::debug!("Config unchanged"),
                                Err(e) => log::error!("Failed to reload config: {}", e),
                            }
                        }
                    }
                    Err(e) => log::error!("Config watch error: {}", e),
                }
            }).expect("Failed to create watcher");
            
            watcher.watch(&path_clone, RecursiveMode::NonRecursive).unwrap();
            
            // Keep thread alive
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        
        Ok(())
    }
    
    fn try_reload_config(
        path: &str,
        config: &Arc<Mutex<Config>>,
        last_modified: &Arc<Mutex<SystemTime>>,
    ) -> Result<bool> {
        let metadata = fs::metadata(path)?;
        let modified = metadata.modified()?;
        
        let mut last_mod = lock_mutex(last_modified, "config_manager.try_reload.last_modified");
        if &modified > &*last_mod {
            *last_mod = modified;
            
            match Config::load(path) {
                Ok(new_config) => {
                    let mut config_lock = lock_mutex(config, "config_manager.try_reload.config");
                    *config_lock = new_config;
                    Ok(true)
                }
                Err(e) => {
                    log::error!("Failed to parse new config: {}", e);
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }
}
