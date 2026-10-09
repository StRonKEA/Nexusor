//! Discovers plugin manifests without requiring external runtime evaluation.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine};

use super::{
    descriptor::{
        AddMethodDefinition, ImportDefinition, PluginModuleDefinition, ProviderDefinition,
        ResourceActionDefinition, ResourceDefinition,
    },
    manifest::PluginManifest,
};
use crate::{config, Error, Result};

const MANIFEST_FILE_NAME: &str = "plugin.json";
const MAX_ICON_BYTES: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct PluginCatalog {
    roots: Vec<PathBuf>,
    app_version: String,
}

#[derive(Clone)]
pub(crate) struct PluginEntry {
    pub manifest: PluginManifest,
    pub definition: PluginModuleDefinition,
    pub icon: String,
}

impl PluginCatalog {
    #[cfg(test)]
    pub(super) fn for_test() -> Self {
        Self {
            roots: Vec::new(),
            app_version: "test".into(),
        }
    }

    pub fn managed(app_version: String) -> Result<Self> {
        let installed = config::managed_data_dir()?.join("plugins/installed");
        fs::create_dir_all(&installed)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&installed, fs::Permissions::from_mode(0o700))?;
        }
        super::builtin::install(&installed)?;

        #[cfg(debug_assertions)]
        let roots = vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/build-in"),
            installed,
        ];
        #[cfg(not(debug_assertions))]
        let roots = vec![installed];
        Ok(Self { roots, app_version })
    }

    pub(crate) async fn entries(&self, _executable: &Path) -> Vec<PluginEntry> {
        let mut plugins = BTreeMap::new();

        // 1. Builtin native providers always registered directly without filesystem scanning
        for id in [
            "dev.nexusor.plugins.antigravity-auth",
            "dev.nexusor.examples.codex-auth",
            "dev.nexusor.examples.grok-auth",
            "dev.nexusor.plugins.github-copilot",
            "dev.nexusor.plugins.kimi-auth",
            "dev.nexusor.plugins.claude-code",
            "dev.nexusor.plugins.nvidia-nim",
            "dev.nexusor.plugins.opencode",
            "dev.nexusor.plugins.groq-lpu",
        ] {
            let entry = builtin_entry(id);
            plugins.insert(entry.manifest.id.clone(), entry);
        }

        // 2. Additional disk plugins if present
        for root in &self.roots {
            let mut directories = match child_directories(root) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(path = %root.display(), %error, "failed to scan plugin directory");
                    continue;
                }
            };
            directories.sort();
            for directory in directories {
                match load_plugin(&directory, &self.app_version).await {
                    Ok(entry) => {
                        if plugins.contains_key(&entry.manifest.id) {
                            tracing::warn!(plugin = %entry.manifest.id, path = %directory.display(), "ignoring duplicate plugin");
                        } else {
                            plugins.insert(entry.manifest.id.clone(), entry);
                        }
                    }
                    Err(error) => {
                        tracing::warn!(path = %directory.display(), %error, "ignoring invalid plugin")
                    }
                }
            }
        }
        plugins.into_values().collect()
    }
}

fn child_directories(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut directories = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
            directories.push(entry.path());
        }
    }
    Ok(directories)
}

fn require_app_version(manifest: &PluginManifest, app_version: &str) -> Result<()> {
    let Some(minimum) = &manifest.min_app_version else {
        return Ok(());
    };
    if super::manifest::version_at_least(app_version, minimum) {
        return Ok(());
    }
    Err(Error::Config(format!(
        "plugin '{}' requires app version {minimum} or newer (current {app_version})",
        manifest.id
    )))
}

async fn load_plugin(directory: &Path, app_version: &str) -> Result<PluginEntry> {
    let manifest: PluginManifest =
        serde_json::from_slice(&fs::read(directory.join(MANIFEST_FILE_NAME))?)?;
    manifest.validate(directory)?;
    require_app_version(&manifest, app_version)?;
    let icon = icon_data_url(directory, &manifest.icon)?;
    let definition = native_definition_for(&manifest.id);

    Ok(PluginEntry {
        manifest,
        definition,
        icon,
    })
}

fn native_definition_for(plugin_id: &str) -> PluginModuleDefinition {
    match plugin_id {
        "dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "codex".into(),
                display_name: serde_json::json!("OpenAI Codex"),
                description: serde_json::json!(
                    "ChatGPT subscription access through official Responses API."
                ),
                provider_type: "openai".into(),
                resource_type: Some("chatgpt-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "chatgpt-account".into(),
                display_name: serde_json::json!("ChatGPT Account"),
                add: vec![AddMethodDefinition {
                    method_type: "oauth2.0".into(),
                    id: "chatgpt-device".into(),
                    display_name: serde_json::json!("Sign in with ChatGPT"),
                    description: serde_json::json!("Authorize this device with OpenAI."),
                    callback: None,
                }],
                import: Some(ImportDefinition {
                    display_name: serde_json::json!("Import ChatGPT Accounts"),
                    description: serde_json::json!("Import accounts from JSON files exported by Nexusor."),
                    accept: vec![".json".into()],
                    multiple: true,
                }),
                actions: vec![
                    ResourceActionDefinition {
                        id: "wakeup-account".into(),
                        display_name: serde_json::json!("Hesabı Uyandır (Wake-up)"),
                        description: serde_json::json!(
                            "Kullanım kotasını günceller ve hesap sayacını hazır tutar."
                        ),
                        target: "resource".into(),
                        destructive: false,
                    },
                    ResourceActionDefinition {
                        id: "redeem-reset-credit".into(),
                        display_name: serde_json::json!("Kotayı Sıfırla (Jeton Kullan)"),
                        description: serde_json::json!(
                            "OpenAI hesabındaki sıfırlama jetonunu kullanarak kotayı anında %100'e getirir."
                        ),
                        target: "resource".into(),
                        destructive: false,
                    },
                ],
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "grok".into(),
                display_name: serde_json::json!("Grok"),
                description: serde_json::json!("Grok subscription access via xAI API."),
                provider_type: "openai".into(),
                resource_type: Some("grok-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "grok-account".into(),
                display_name: serde_json::json!("xAI Account"),
                add: vec![AddMethodDefinition {
                    method_type: "oauth2.0".into(),
                    id: "grok-device".into(),
                    display_name: serde_json::json!("Sign in with xAI"),
                    description: serde_json::json!("Authorize this device with xAI."),
                    callback: None,
                }],
                import: Some(ImportDefinition {
                    display_name: serde_json::json!("Import xAI Accounts"),
                    description: serde_json::json!("Import accounts from JSON files exported by Nexusor."),
                    accept: vec![".json".into()],
                    multiple: true,
                }),
                actions: Vec::new(),
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "antigravity".into(),
                display_name: serde_json::json!("Google Antigravity"),
                description: serde_json::json!("Google Cloud Code models."),
                provider_type: "google".into(),
                resource_type: Some("google-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "google-account".into(),
                display_name: serde_json::json!("Google Account"),
                add: vec![AddMethodDefinition {
                    method_type: "oauth2.authorization-code".into(),
                    id: "google-oauth".into(),
                    display_name: serde_json::json!("Sign in with Google"),
                    description: serde_json::json!("Authorize this device with Google."),
                    callback: None,
                }],
                import: Some(ImportDefinition {
                    display_name: serde_json::json!("Import Google Accounts"),
                    description: serde_json::json!("Import accounts from JSON files exported by Nexusor."),
                    accept: vec![".json".into()],
                    multiple: true,
                }),
                actions: vec![ResourceActionDefinition {
                    id: "wakeup-account".into(),
                    display_name: serde_json::json!("Hesabı Uyandır (Wake-up)"),
                    description: serde_json::json!(
                        "Google Cloud Code modeline hafif bir uyandırma isteği göndererek 5 saatlik kota sıfırlama sayacını erkenden başlatır."
                    ),
                    target: "resource".into(),
                    destructive: false,
                }],
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.plugins.github-copilot" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "copilot".into(),
                display_name: serde_json::json!("GitHub Copilot"),
                description: serde_json::json!("GitHub Copilot models (Claude 3.5/3.7 Sonnet, GPT-4o, o3-mini)."),
                provider_type: "openai_chat".into(),
                resource_type: Some("github-copilot-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "github-copilot-account".into(),
                display_name: serde_json::json!("GitHub Account"),
                add: vec![AddMethodDefinition {
                    method_type: "oauth2.device-code".into(),
                    id: "github-copilot-device".into(),
                    display_name: serde_json::json!("Sign in with GitHub"),
                    description: serde_json::json!("Authorize this device with GitHub Copilot."),
                    callback: None,
                }],
                import: None,
                actions: vec![
                    ResourceActionDefinition {
                        id: "ping-account".into(),
                        display_name: serde_json::json!("Bağlantıyı Test Et"),
                        description: serde_json::json!("GitHub Copilot abonelik durumunu ve API erişimini test eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                ],
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.plugins.kimi-auth" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "kimi".into(),
                display_name: serde_json::json!("Moonshot Kimi"),
                description: serde_json::json!("Kimi Code Plan models with 1M context and reasoning."),
                provider_type: "openai_chat".into(),
                resource_type: Some("kimi-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "kimi-account".into(),
                display_name: serde_json::json!("Kimi Account"),
                add: vec![AddMethodDefinition {
                    method_type: "oauth2.device-code".into(),
                    id: "kimi-device".into(),
                    display_name: serde_json::json!("Sign in with Kimi"),
                    description: serde_json::json!("Authorize this device with your Kimi account."),
                    callback: None,
                }],
                import: None,
                actions: vec![
                    ResourceActionDefinition {
                        id: "ping-account".into(),
                        display_name: serde_json::json!("Bağlantıyı Test Et"),
                        description: serde_json::json!("Moonshot Kimi API bağlantısını ve yanıt süresini anında test eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                    ResourceActionDefinition {
                        id: "wakeup-account".into(),
                        display_name: serde_json::json!("Hesabı Uyandır (Wake-up)"),
                        description: serde_json::json!("Kimi Code Plan kotalarını anında senkronize eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                ],
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.plugins.claude-code" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "claude-code".into(),
                display_name: serde_json::json!("Claude Code"),
                description: serde_json::json!("Claude Code CLI subscription models (Claude 3.7 Sonnet, 3.5 Sonnet, Haiku, Opus)."),
                provider_type: "anthropic".into(),
                resource_type: Some("claude-code-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "claude-code-account".into(),
                display_name: serde_json::json!("Claude Account"),
                add: vec![AddMethodDefinition {
                    method_type: "oauth2.authorization-code".into(),
                    id: "claude-code-oauth".into(),
                    display_name: serde_json::json!("Sign in with Claude"),
                    description: serde_json::json!("Authorize with your Claude account (Claude Pro / Team / Code)."),
                    callback: None,
                }],
                import: Some(ImportDefinition {
                    display_name: serde_json::json!("Import Local Claude Code Session"),
                    description: serde_json::json!("Import session from ~/.claude/.credentials.json or JSON file."),
                    accept: vec![".json".into()],
                    multiple: false,
                }),
                actions: vec![
                    ResourceActionDefinition {
                        id: "ping-account".into(),
                        display_name: serde_json::json!("Bağlantıyı Test Et"),
                        description: serde_json::json!("Claude Code API bağlantısını ve yanıt süresini anında test eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                    ResourceActionDefinition {
                        id: "wakeup-account".into(),
                        display_name: serde_json::json!("Hesabı Uyandır (Wake-up)"),
                        description: serde_json::json!("Claude Code kotalarını ve oturumunu anında senkronize eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                ],
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.plugins.nvidia-nim" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "nvidia-nim".into(),
                display_name: serde_json::json!("NVIDIA NIM"),
                description: serde_json::json!("NVIDIA NIM Microservices (DeepSeek R1, Llama 3.3, Qwen 2.5 Coder)."),
                provider_type: "openai_chat".into(),
                resource_type: Some("nvidia-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "nvidia-account".into(),
                display_name: serde_json::json!("NVIDIA Account"),
                add: vec![AddMethodDefinition {
                    method_type: "api_key".into(),
                    id: "nvidia-key".into(),
                    display_name: serde_json::json!("API Anahtarı ile Bağla"),
                    description: serde_json::json!("build.nvidia.com API anahtarınızı girin."),
                    callback: None,
                }],
                import: Some(ImportDefinition {
                    display_name: serde_json::json!("Import NVIDIA Accounts"),
                    description: serde_json::json!("Import accounts from JSON files exported by Nexusor."),
                    accept: vec![".json".into()],
                    multiple: true,
                }),
                actions: vec![
                    ResourceActionDefinition {
                        id: "ping-account".into(),
                        display_name: serde_json::json!("Bağlantıyı Test Et"),
                        description: serde_json::json!("NVIDIA NIM API bağlantısını ve yanıt süresini anında test eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                ],
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.plugins.opencode" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "opencode".into(),
                display_name: serde_json::json!("OpenCode"),
                description: serde_json::json!("OpenCode Open-Source Coding Agent & Model Ecosystem."),
                provider_type: "openai_chat".into(),
                resource_type: Some("opencode-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "opencode-account".into(),
                display_name: serde_json::json!("OpenCode Account"),
                add: vec![AddMethodDefinition {
                    method_type: "api_key".into(),
                    id: "opencode-key".into(),
                    display_name: serde_json::json!("API Anahtarı ile Bağla"),
                    description: serde_json::json!("OpenCode API anahtarınızı veya özel uç nokta bilginizi girin."),
                    callback: None,
                }],
                import: Some(ImportDefinition {
                    display_name: serde_json::json!("Import OpenCode Accounts"),
                    description: serde_json::json!("Import accounts from JSON files exported by Nexusor."),
                    accept: vec![".json".into()],
                    multiple: true,
                }),
                actions: vec![
                    ResourceActionDefinition {
                        id: "ping-account".into(),
                        display_name: serde_json::json!("Bağlantıyı Test Et"),
                        description: serde_json::json!("OpenCode API bağlantısını ve yanıt süresini anında test eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                ],
                can_refresh: true,
                can_remove: true,
            }],
        },
        "dev.nexusor.plugins.groq-lpu" => PluginModuleDefinition {
            providers: vec![ProviderDefinition {
                id: "groq".into(),
                display_name: serde_json::json!("Groq"),
                description: serde_json::json!("Groq LPU Inference Engine (DeepSeek R1, Llama 3.3)."),
                provider_type: "openai_chat".into(),
                resource_type: Some("groq-lpu-account".into()),
                has_models: true,
            }],
            resources: vec![ResourceDefinition {
                resource_type: "groq-lpu-account".into(),
                display_name: serde_json::json!("Groq Account"),
                add: vec![AddMethodDefinition {
                    method_type: "api_key".into(),
                    id: "groq-key".into(),
                    display_name: serde_json::json!("Groq API Anahtarı ile Bağla"),
                    description: serde_json::json!("console.groq.com API anahtarınızı girin."),
                    callback: None,
                }],
                import: Some(ImportDefinition {
                    display_name: serde_json::json!("Import Groq Accounts"),
                    description: serde_json::json!("Import accounts from JSON files exported by Nexusor."),
                    accept: vec![".json".into()],
                    multiple: true,
                }),
                actions: vec![
                    ResourceActionDefinition {
                        id: "ping-account".into(),
                        display_name: serde_json::json!("Bağlantıyı Test Et"),
                        description: serde_json::json!("Groq LPU API bağlantısını ve yanıt süresini anında test eder."),
                        target: "resource".into(),
                        destructive: false,
                    },
                ],
                can_refresh: true,
                can_remove: true,
            }],
        },
        _ => PluginModuleDefinition {
            providers: Vec::new(),
            resources: Vec::new(),
        },
    }
}

const ANTIGRAVITY_ICON: &str = include_str!("../../icons/antigravity.svg");
const CODEX_ICON: &str = include_str!("../../icons/codex.svg");
const GROK_ICON: &str = include_str!("../../icons/grok.svg");
const COPILOT_ICON: &str = include_str!("../../icons/copilot.svg");
const KIMI_ICON: &str = include_str!("../../icons/kimi.svg");
const CLAUDE_CODE_ICON: &str = include_str!("../../icons/claude-code.svg");
const NVIDIA_ICON: &str = include_str!("../../icons/nvidia.svg");
const OPENCODE_ICON: &str = include_str!("../../icons/opencode.svg");
const GROQ_ICON: &str = include_str!("../../icons/groq.svg");

fn builtin_icon_data_url(svg_content: &str) -> String {
    format!(
        "data:image/svg+xml;base64,{}",
        STANDARD.encode(svg_content.as_bytes())
    )
}

fn builtin_entry(plugin_id: &str) -> PluginEntry {
    let (name, icon) = match plugin_id {
        "dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth" => (
            "Google Antigravity",
            builtin_icon_data_url(ANTIGRAVITY_ICON),
        ),
        "dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth" => {
            ("OpenAI Codex", builtin_icon_data_url(CODEX_ICON))
        }
        "dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth" => {
            ("xAI Grok", builtin_icon_data_url(GROK_ICON))
        }
        "dev.nexusor.plugins.github-copilot" => {
            ("GitHub Copilot", builtin_icon_data_url(COPILOT_ICON))
        }
        "dev.nexusor.plugins.kimi-auth" => ("Moonshot Kimi", builtin_icon_data_url(KIMI_ICON)),
        "dev.nexusor.plugins.claude-code" => {
            ("Claude Code", builtin_icon_data_url(CLAUDE_CODE_ICON))
        }
        "dev.nexusor.plugins.nvidia-nim" => ("NVIDIA NIM", builtin_icon_data_url(NVIDIA_ICON)),
        "dev.nexusor.plugins.opencode" => ("OpenCode", builtin_icon_data_url(OPENCODE_ICON)),
        "dev.nexusor.plugins.groq-lpu" => ("Groq", builtin_icon_data_url(GROQ_ICON)),
        _ => (plugin_id, String::new()),
    };

    let manifest = PluginManifest {
        api_version: 1,
        id: plugin_id.to_string(),
        name: name.to_string(),
        version: "1.0.0".to_string(),
        author: Some("Nexusor".to_string()),
        min_app_version: None,
        icon: icon.clone(),
        entry: String::new(),
        permissions: Default::default(),
    };

    let definition = native_definition_for(plugin_id);

    PluginEntry {
        manifest,
        definition,
        icon,
    }
}

fn icon_data_url(directory: &Path, icon_path: &str) -> Result<String> {
    let path = directory.join(icon_path);
    let bytes = fs::read(&path)?;
    if bytes.len() as u64 > MAX_ICON_BYTES {
        return Err(Error::Config(format!(
            "plugin icon {} exceeds 1MB",
            path.display()
        )));
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("svg");
    let mime = if ext.eq_ignore_ascii_case("svg") {
        "image/svg+xml"
    } else if ext.eq_ignore_ascii_case("png") {
        "image/png"
    } else {
        "application/octet-stream"
    };
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)))
}
