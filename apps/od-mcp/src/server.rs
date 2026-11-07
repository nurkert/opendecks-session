//! The MCP server handler: tools, resources and prompts over one library.

use crate::tools::{self, ToolDef};
use crate::view::track_full;
use od_core::TrackId;
use od_library::Library;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, GetPromptRequestParams,
    GetPromptResponse, GetPromptResult, Implementation, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    Prompt, PromptArgument, PromptMessage, ProtocolVersion, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ResourceTemplate, Role,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::Value;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

/// The protocol revision this server implements.
pub const PROTOCOL: ProtocolVersion = ProtocolVersion::V_2025_06_18;

const INSTRUCTIONS: &str = "\
OpenDecks Session library: prepare DJ sets with the DJ. Start with library_overview. \
Find tracks with search_tracks and harmonic_neighbours, read details with get_tracks, \
check an order with set_flow. Keys are Camelot (8A = A minor); positions in lists are 0-based. \
Every write is one undo step labelled \"MCP: …\" in the app (Edit > Undo), and the app shows it live. \
Propose bigger changes before making them; delete_node needs confirm=true and the DJ's consent. \
The DMX light show is here too: lighting_overview shows the rig; lighting_patch, lighting_settings, lighting_group, \
lighting_output, lighting_cue and lighting_track_cues edit it (the app follows at once), lighting_live acts on the running show.";

#[derive(Clone)]
pub struct OpenDecksServer {
    lib: Arc<Mutex<Library>>,
    tools: Arc<Vec<ToolDef>>,
}

impl OpenDecksServer {
    pub fn new(lib: Library) -> Self {
        Self {
            lib: Arc::new(Mutex::new(lib)),
            tools: Arc::new(tools::all()),
        }
    }

    fn with_lib<T>(&self, f: impl FnOnce(&mut Library) -> T) -> T {
        let mut guard = self.lib.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    }

    /// Runs a tool by name (also used by tests without a transport).
    pub fn call(&self, name: &str, args: Value) -> Option<CallToolResult> {
        let tool = self.tools.iter().find(|t| t.name == name)?;
        let res = self.with_lib(|lib| tool.run(lib, args));
        Some(match res {
            Ok(o) => {
                let json = serde_json::to_string(&o.data).unwrap_or_default();
                let mut r = CallToolResult::success(vec![
                    ContentBlock::text(o.summary),
                    ContentBlock::text(json),
                ]);
                r.structured_content = Some(o.data);
                r
            }
            Err(msg) => CallToolResult::error(vec![ContentBlock::text(msg)]),
        })
    }

    fn read_uri(&self, uri: &str) -> Result<String, McpError> {
        let bad = || McpError::resource_not_found(format!("unknown resource {uri}"), None);
        let rest = uri.strip_prefix("opendecks://").ok_or_else(bad)?;
        let (kind, id) = rest.split_once('/').ok_or_else(bad)?;
        let id: i64 = id.parse().map_err(|_| bad())?;
        let v = self.with_lib(|lib| match kind {
            "track" => lib
                .track(TrackId(id))
                .ok()
                .and_then(|t| track_full(lib, &t).ok()),
            "node" => tools::node_json(lib, id, 500).ok(),
            _ => None,
        });
        v.map(|v| serde_json::to_string(&v).unwrap_or_default())
            .ok_or_else(bad)
    }
}

fn prompt_text(name: &str, args: &serde_json::Map<String, Value>) -> Option<(String, String)> {
    let arg = |k: &str| {
        args.get(k)
            .and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| Some(v.to_string()))
            })
            .filter(|s| !s.is_empty() && s != "null")
    };
    match name {
        "build_set" => {
            let theme = arg("theme").unwrap_or_else(|| "(ask me)".into());
            let duration = arg("duration_minutes").unwrap_or_else(|| "60".into());
            let arc = arg("energy_arc").unwrap_or_else(|| "warm-up, build, peak, cool-down".into());
            let bpm = arg("bpm_range").unwrap_or_else(|| "(pick from the library)".into());
            Some((
                "Plan and create a set in the OpenDecks library".into(),
                format!(
                    "Help me prepare a DJ set in my OpenDecks library.\n\
                     Theme / feeling: {theme}\nLength: about {duration} minutes\nEnergy arc: {arc}\nBPM range: {bpm}\n\n\
                     How to work:\n\
                     1. Call library_overview to see genres, BPM/key spread, My Tag values and existing sets.\n\
                     2. Find candidates with search_tracks (bpm range, genres, energy, rating, My Tag); prefer tracks I rated well and haven't overplayed.\n\
                     3. Chain the order with harmonic_neighbours (Camelot ±1 / relative, tempo steps under ~6 %) so the energy follows the arc; use get_tracks for phrases (intro/outro lengths) and cues when planning mixes.\n\
                     4. Show me the draft order (title, artist, BPM, key, energy, running time) and wait for my OK.\n\
                     5. Then create_set with the tracks in order, run set_flow, fix the warnings, and write a set_transition_note for each mix (where to start the next track, which phrase, EQ / effect idea).\n\
                     Every change is one undo step in the app, so I can roll back anything."
                ),
            ))
        }
        "fix_transitions" => {
            let node = arg("node_id")?;
            Some((
                "Find and fix weak transitions in a set".into(),
                format!(
                    "Check the transitions of my set or playlist with id {node} in OpenDecks.\n\
                     1. Call set_flow for it and get_node for the existing notes.\n\
                     2. For every warning (tempo jump, key clash, energy jump/drop, repeats, same artist), propose a fix: reorder, swap in a track from harmonic_neighbours (exclude_node = {node}), or a transition technique, keeping the overall energy arc.\n\
                     3. List the proposed changes and wait for my OK.\n\
                     4. Apply them (reorder / add_tracks / remove_entries), write set_transition_note for the changed mixes, and run set_flow again to confirm."
                ),
            ))
        }
        _ => None,
    }
}

impl ServerHandler for OpenDecksServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_protocol_version(PROTOCOL)
        .with_server_info(Implementation::new(
            "opendecks-session",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(INSTRUCTIONS)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(ProtocolVersion::known_up_to(&PROTOCOL).to_vec())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(
            self.tools.iter().map(ToolDef::to_tool).collect(),
        ))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        self.tools
            .iter()
            .find(|t| t.name == name)
            .map(ToolDef::to_tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.map(Value::Object).unwrap_or(Value::Null);
        match self.call(&request.name, args) {
            Some(r) => Ok(r.into()),
            None => Err(McpError::invalid_params(
                format!("unknown tool {}", request.name),
                None,
            )),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let nodes = self.with_lib(|lib| lib.all_nodes().unwrap_or_default());
        let items = nodes
            .iter()
            .filter(|n| n.kind != od_library::NodeKind::Folder)
            .take(500)
            .map(|n| {
                Resource::new(format!("opendecks://node/{}", n.id), n.name.clone())
                    .with_description(format!("OpenDecks {}", n.kind.as_str()))
                    .with_mime_type("application/json")
            })
            .collect();
        Ok(ListResourcesResult::with_all_items(items))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new("opendecks://track/{id}", "track")
                .with_description("Every field of one track (same as get_tracks)")
                .with_mime_type("application/json"),
            ResourceTemplate::new("opendecks://node/{id}", "node")
                .with_description(
                    "A playlist, set, smart list or folder with its entries (same as get_node)",
                )
                .with_mime_type("application/json"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let text = self.read_uri(&request.uri)?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, request.uri.clone()).with_mime_type("application/json"),
        ])
        .into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let a = |n: &str, d: &str, req: bool| {
            PromptArgument::new(n)
                .with_description(d)
                .with_required(req)
        };
        Ok(ListPromptsResult::with_all_items(vec![
            Prompt::new(
                "build_set",
                Some("Plan a set with the DJ and create it in the library"),
                Some(vec![
                    a(
                        "theme",
                        "Theme or feeling, e.g. \"sunset rooftop, deep and warm\"",
                        false,
                    ),
                    a("duration_minutes", "Length of the set in minutes", false),
                    a(
                        "energy_arc",
                        "Energy shape, e.g. \"slow build to a peak at 75 %, short cool-down\"",
                        false,
                    ),
                    a("bpm_range", "BPM range, e.g. \"118-124\"", false),
                ]),
            ),
            Prompt::new(
                "fix_transitions",
                Some("Find clashing transitions in a set and fix them"),
                Some(vec![a(
                    "node_id",
                    "Id of the set or playlist (from list_nodes)",
                    true,
                )]),
            ),
        ]))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let args = request.arguments.clone().unwrap_or_default();
        let (desc, text) = prompt_text(&request.name, &args).ok_or_else(|| {
            McpError::invalid_params(
                format!("unknown prompt {} or missing argument", request.name),
                None,
            )
        })?;
        Ok(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
                .with_description(desc)
                .into(),
        )
    }
}
