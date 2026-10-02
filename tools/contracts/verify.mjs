import fs from "node:fs";
import path from "node:path";

const root=process.cwd();
const read=(p)=>JSON.parse(fs.readFileSync(path.join(root,p),"utf8"));
const exists=(p)=>fs.existsSync(path.join(root,p));
const fail=(m)=>{throw new Error(m)};

const mcf=read("schemas/mcf-v2/manifest.json");
for(const file of mcf.schemas ?? []) if(!exists("schemas/mcf-v2/"+file)) fail("Missing MCF schema: "+file);
for(const file of mcf.required_files ?? []) if(!exists("schemas/mcf-v2/"+file)) fail("Missing required MCF file: "+file);

const messages=read("schemas/mcf-v2/message-types.schema.json").enum;
const events=read("schemas/mcf-v2/event-types.schema.json").enum;
const registry=read("schemas/mcf-v2/registry.json");
if(messages.length!==new Set(messages).size) fail("Duplicate MCF message type");
if(events.length!==new Set(events).size) fail("Duplicate MCF event type");
for(const name of messages) if(!registry.message_to_payload?.[name]) fail("No payload mapping for message: "+name);
for(const name of messages) if(!registry.message_priority?.[name]) fail("No priority mapping for message: "+name);

const transition=read("schemas/mcf-v2/transition-types.json");
for(const [machine,def] of Object.entries(transition.machines)){
  const ids=new Set((transition.transitions?.[machine]??[]).map(t=>t.transition_id));
  for(let i=0;i<def.states.length-1;i++) {
    const id=`${machine}.${def.states[i]}->${def.states[i+1]}`;
    if(!ids.has(id)) fail(`Missing adjacent transition: ${id}`);
  }
}

const bridge=read("schemas/tauri-bridge-v1/bridge.schema.json");
const workspace=read("workspace.manifest.json");
for(const c of bridge.properties.command.enum) if(!workspace.tauri_bridge.commands?.[c]) fail("No command owner: "+c);
for(const q of bridge.properties.query.enum) if(!workspace.tauri_bridge.queries?.[q]) fail("No query owner: "+q);
for(const e of bridge.properties.event_type.enum) if(!workspace.tauri_bridge.events?.[e]) fail("No event owner: "+e);

const payloadRegistry=read("schemas/tauri-bridge-v1/payloads.json");
for(const c of bridge.properties.command.enum) if(!payloadRegistry.commands?.[c]) fail("No command payload metadata: "+c);
for(const q of bridge.properties.query.enum) if(!payloadRegistry.queries?.[q]) fail("No query payload metadata: "+q);
for(const e of bridge.properties.event_type.enum) if(!payloadRegistry.events?.[e]) fail("No event payload metadata: "+e);

const requiredRefs=workspace.schema_sources ?? [];
for(const p of requiredRefs) if(!exists(p)) fail("Missing workspace schema source: "+p);

// --- Agent adapter set (DEC-029): the closed agent set must not drift between schemas. ---
const canonicalAgents=["HERMES_AGENT","KILO_CODE","OPEN_CODE"];
const sameSet=(a,b)=>a.length===b.length&&canonicalAgents.every(x=>a.includes(x)&&b.includes(x));
const agentEnums=[
  ["schemas/agent-adapter-v1/adapter-types.schema.json",s=>s.properties.types.properties.AgentInstallation.properties.agent_type.enum],
  ["schemas/agent-adapter-v1/native-event.schema.json",s=>s.properties.agent_type.enum],
  ["schemas/agent-adapter-v1/probe-result.schema.json",s=>s.properties.agent_type.enum],
  ["schemas/doctor-v1/doctor-report.schema.json",s=>s.properties.agents.items.properties.agent_type.enum],
  ["schemas/mcf-v2/handshake.schema.json",s=>s.properties.agent_type.enum],
  ["schemas/mcf-v2/identity.schema.json",s=>s.properties.agent_type.enum.filter(x=>x!==null)],
];
for(const [file,pick] of agentEnums){
  const got=pick(read(file));
  if(!sameSet(got,canonicalAgents)) fail(`Agent set drift in ${file}: [${got.join(",")}] != [${canonicalAgents.join(",")}]`);
}
const contract=read("schemas/agent-adapter-v1/native-transport-contract.json");
const contractAgents=Object.keys(contract.agents);
if(!sameSet(contractAgents,canonicalAgents)) fail(`Transport contract agent set drift: [${contractAgents.join(",")}]`);
for(const [name,def] of Object.entries(contract.agents)){
  if(!def.transport) fail(`Transport contract missing transport for ${name}`);
}
const transportEnum=read("schemas/agent-adapter-v1/probe-result.schema.json").properties.transport.enum;
for(const name of canonicalAgents){
  const t=contract.agents[name].transport;
  if(!transportEnum.includes(t)) fail(`Contract transport ${t} (${name}) is not in probe-result transport enum`);
}
// The conformance YAML declares itself subordinate to the transport contract. No YAML parser is
// available (this repo has zero dependencies), so guard the one dimension that actually drifts:
// the agent set. Every canonical agent must appear as a top-level key, and no other agent may.
const capYaml=fs.readFileSync(path.join(root,"schemas/mcf-v2/conformance/adapter-capabilities.yaml"),"utf8");
for(const name of canonicalAgents){
  if(!new RegExp(`^  ${name}:`,"m").test(capYaml)) fail(`Conformance YAML missing agent block: ${name}`);
}
const capAgentBlocks=[...capYaml.matchAll(/^  ([A-Z][A-Z0-9_]*):/gm)].map(m=>m[1]);
for(const name of capAgentBlocks) if(!canonicalAgents.includes(name)) fail(`Conformance YAML declares non-canonical agent: ${name}`);

// native_kind enum and the native->MCF mapping keys must stay in lockstep.
const nativeKinds=read("schemas/agent-adapter-v1/native-event.schema.json").properties.native_kind.enum;
const nativeMap=read("schemas/agent-adapter-v1/native-to-mcf.registry.json").mappings;
for(const k of nativeKinds) if(!(k in nativeMap)) fail(`native_kind ${k} has no native-to-MCF mapping`);
for(const k of Object.keys(nativeMap)) if(!nativeKinds.includes(k)) fail(`native-to-MCF mapping ${k} is not a native_kind`);
for(const [k,v] of Object.entries(nativeMap)){
  if(v!==null && !messages.includes(v)) fail(`native-to-MCF mapping ${k} -> ${v} is not an MCF message type`);
}

console.log("Mayasaba contract verification passed.");
console.log(`MCF messages: ${messages.length}; events: ${events.length}; transition machines: ${Object.keys(transition.machines).length}`);
console.log(`Tauri commands: ${bridge.properties.command.enum.length}; queries: ${bridge.properties.query.enum.length}; UI events: ${bridge.properties.event_type.enum.length}`);

const wm=read("workspace.manifest.json");
if(!exists("Cargo.toml") || !exists("package.json")) fail("Missing root workspace manifest");
for(const member of wm.rust_workspace.members){
  if(!exists(member + "/Cargo.toml")) fail("Missing Rust manifest: " + member + "/Cargo.toml");
}
if(!exists(wm.desktop.path + "/package.json")) fail("Missing desktop package manifest");
if(!exists(wm.tauri_shell.path + "/tauri.conf.json")) fail("Missing Tauri configuration");
for(const [name,def] of Object.entries(wm.crates)){
  const actual=(def.depends||[]).map(x=>"mayasaba-"+x);
  const cargo=fs.readFileSync(path.join(root,def.path,"Cargo.toml"),"utf8");
  for(const dep of actual) if(!cargo.includes(dep)) fail("Manifest missing declared dependency: "+name+" -> "+dep);
}
