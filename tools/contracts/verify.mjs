import fs from "node:fs";
import path from "node:path";

const root=process.cwd();
const read=(p)=>JSON.parse(fs.readFileSync(path.join(root,p),"utf8"));
const exists=(p)=>fs.existsSync(path.join(root,p));
const fail=(m)=>{throw new Error(m)};

const mcf=read("schemas/mcf-v2/manifest.json");
for(const file of mcf.schemas ?? []) if(!exists("schemas/mcf-v2/"+file)) fail("Missing MCF schema: "+file);

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

console.log("Mayasaba contract verification passed.");
console.log(`MCF messages: ${messages.length}; events: ${events.length}; transition machines: ${Object.keys(transition.machines).length}`);
console.log(`Tauri commands: ${bridge.properties.command.enum.length}; queries: ${bridge.properties.query.enum.length}; UI events: ${bridge.properties.event_type.enum.length}`);
