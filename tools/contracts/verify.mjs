import fs from "node:fs";
import path from "node:path";
import {execFileSync} from "node:child_process";

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

// --- Message-set cross-checks, the symmetric counterpart to the event checks below. DEC-033 claims
// contract verification enforces all four message registry updates a new message type requires
// (`message_types`, `message_to_payload`, `message_priority`, `message-payloads.registry.json`); as
// written the gate read only two of the four, and read them one-directionally — it checked that every
// enum member had a mapping, never that a list contained nothing the enum does not. Both directions
// and all four lists are checked now, so the record's claim becomes true rather than needing to be
// re-scoped.
const messageLists=[
  ["schemas/mcf-v2/registry.json","message_types",registry.message_types],
  ["schemas/mcf-v2/registry.json","message_to_payload",Object.keys(registry.message_to_payload??{})],
  ["schemas/mcf-v2/registry.json","message_priority",Object.keys(registry.message_priority??{})],
  ["schemas/mcf-v2/message-payloads.registry.json","messages",Object.keys(read("schemas/mcf-v2/message-payloads.registry.json").messages??{})],
];
for(const [file,key,list] of messageLists){
  if(!Array.isArray(list)) fail(`Message list ${file}:${key} is missing or not a list`);
  const missing=messages.filter(m=>!list.includes(m));
  const extra=list.filter(m=>!messages.includes(m));
  if(missing.length) fail(`Message list ${file}:${key} omits ${missing.length} message(s) present in message-types.schema.json: ${missing.join(", ")}`);
  if(extra.length) fail(`Message list ${file}:${key} names ${extra.length} message(s) absent from message-types.schema.json: ${extra.join(", ")}`);
  const dupes=list.filter((m,i)=>list.indexOf(m)!==i);
  if(dupes.length) fail(`Message list ${file}:${key} repeats ${dupes.length} entry/entries: ${[...new Set(dupes)].join(", ")}`);
}

// --- Event-set cross-checks. Four separate lists enumerate the event set, but until now only the
// event-types.schema.json enum was read. registry.json's `event_types` and `event_to_ui`, and
// event-to-ui.registry.json's `mapping`, each declare themselves authoritative (the last two
// explicitly, via their own `authority` field, under DEC-021) yet no tool read them, so they drifted:
// ADMISSION_RECORDED (DEC-035) reached the enum, the payload registry and registry.event_to_ui, but
// not registry.event_types or event-to-ui.registry.json. Each list must now equal the enum exactly,
// so an event cannot be registered in one place only.
const eventLists=[
  ["schemas/mcf-v2/registry.json","event_types",registry.event_types],
  ["schemas/mcf-v2/registry.json","event_to_ui",Object.keys(registry.event_to_ui??{})],
  ["schemas/mcf-v2/event-to-ui.registry.json","mapping",Object.keys(read("schemas/mcf-v2/event-to-ui.registry.json").mapping??{})],
  ["schemas/mcf-v2/event-payloads.registry.json","events",Object.keys(read("schemas/mcf-v2/event-payloads.registry.json").events??{})],
];
for(const [file,key,list] of eventLists){
  if(!Array.isArray(list)) fail(`Event list ${file}:${key} is missing or not a list`);
  const missing=events.filter(e=>!list.includes(e));
  const extra=list.filter(e=>!events.includes(e));
  if(missing.length) fail(`Event list ${file}:${key} omits ${missing.length} event(s) present in event-types.schema.json: ${missing.join(", ")}`);
  if(extra.length) fail(`Event list ${file}:${key} names ${extra.length} event(s) absent from event-types.schema.json: ${extra.join(", ")}`);
  // A set comparison via includes() cannot see a repeat: a list holding INTENT_RECORDED twice has the
  // same membership as one holding it once, so a duplicated entry passed every check above.
  // This catches repeated array entries only. A repeated *key* inside one of the JSON objects is
  // invisible here, because JSON.parse has already discarded all but the last occurrence — detecting
  // that needs a raw-text scan, which this gate does not do.
  const dupes=list.filter((e,i)=>list.indexOf(e)!==i);
  if(dupes.length) fail(`Event list ${file}:${key} repeats ${dupes.length} entry/entries: ${[...new Set(dupes)].join(", ")}`);
}
// registry.event_to_ui and event-to-ui.registry.json both claim to be the event -> UI projection.
// Equal key sets are not enough: the two must agree on every projection, or the UI event a runtime
// derives depends on which of the two files it happened to read.
// Agreement between the two is also not enough on its own: they could agree on a name that is not a
// Tauri UI event at all. Nothing validated the projection *target*, so setting both sides to a
// nonexistent event passed. Every value must now be a member of the bridge's UI event enum.
const uiEventEnum=new Set(read("schemas/tauri-bridge-v1/bridge.schema.json").properties.event_type.enum);
const uiProjectionA=registry.event_to_ui??{};
const uiProjectionB=read("schemas/mcf-v2/event-to-ui.registry.json").mapping??{};
for(const k of Object.keys(uiProjectionA)){
  if(k in uiProjectionB && uiProjectionA[k]!==uiProjectionB[k]) fail(`event_to_ui projection disagreement for ${k}: registry.json="${uiProjectionA[k]}" vs event-to-ui.registry.json="${uiProjectionB[k]}"`);
}
for(const [src,map] of [["registry.json",uiProjectionA],["event-to-ui.registry.json",uiProjectionB]]){
  for(const [k,v] of Object.entries(map)){
    if(!uiEventEnum.has(v)) fail(`event_to_ui projection in ${src} maps ${k} to "${v}", which is not a Tauri UI event in bridge.schema.json`);
  }
}

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

// --- Consumer coverage. `schema_sources` declares 38 files authoritative, but until now nothing
// checked that any of them was actually consumed; three of them (registry.json, event-to-ui.registry.json,
// event-payloads.registry.json) declared themselves the authority for a mapping and were read by no
// tool at all, which is how the ADMISSION_RECORDED drift survived. Existence is not integration.
// A schema source must be named by at least one tracked file other than itself and other than
// workspace.manifest.json — the manifest lists it, but a listing is not a consumer. "Named" means the
// file's repo-relative path appears in that file's text, or its basename does *and* that basename is
// unique in the repository. The uniqueness condition is load-bearing: six files are named
// `registry.json`, so accepting a bare basename would let one unrelated mention of "registry.json"
// satisfy every `*/registry.json` source and the check would report almost nothing. A basename that
// identifies exactly one file is an unambiguous reference; an ambiguous one is not evidence.
// The match must land on a token boundary, not anywhere inside a longer name. A bare `String.includes`
// is not a reference test: the basename `payloads.schema.json` (unique, count 1) occurs *inside*
// `event-payloads.schema.json` and `message-payloads.schema.json`, so a plain substring test credited
// the wrong file and passed while the source's only real reference was deleted. That was reproduced
// end-to-end before this boundary condition was added. A name is only "named" when the character
// before it is not a filename character — so a path separator (a genuine relative reference such as
// `../validation-v1/failure.schema.json`) still counts, but a `-`, `.`, `_` or alphanumeric glue that
// makes it part of a longer filename does not. The same boundary is applied after the match, so
// `payloads.schema.json.bak` is not a reference either.
// A *documenting* reference counts as integration, deliberately: requiring a code reader would fail on
// every schema whose owning crate is still an unimplemented stub, and the check would then report
// "not built yet" rather than "drifted", which is a different and much noisier claim. Tracked files are
// preferred so an untracked scratch file cannot satisfy the check; if git is unavailable the check
// falls back to a working-tree walk and says so, rather than adding a hard git dependency to the gate.
// Known limit, stated so it is not overclaimed: this proves a source is *named* somewhere, not that the
// naming is load-bearing. A bullet in a documentation inventory satisfies it, so it detects "nothing
// anywhere references this file" — the failure that let the ADMISSION_RECORDED drift survive — and not
// "the reference is actually read". A stronger check needs a declared consumer per source, which the
// manifest does not carry today.
// True only when `name` occurs in `text` as a whole filename token. The character before and after
// the match must not be a filename character (`[A-Za-z0-9._-]`), so `payloads.schema.json` does not
// match inside `event-payloads.schema.json`, while a path-qualified reference such as
// `../validation-v1/failure.schema.json` still matches on its `/` boundary.
const nameChar=/[A-Za-z0-9._-]/;
const namesFile=(text,name)=>{
  for(let i=text.indexOf(name);i!==-1;i=text.indexOf(name,i+1)){
    const before=i===0?null:text[i-1];
    const after=text[i+name.length]??null;
    if((before===null||!nameChar.test(before))&&(after===null||!nameChar.test(after))) return true;
  }
  return false;
};
let trackedFiles=null;
try {
  trackedFiles=execFileSync("git",["ls-files"],{cwd:root,encoding:"utf8",stdio:["ignore","pipe","ignore"]}).trim().split(/\r?\n/).filter(Boolean);
} catch { /* not a git checkout — fall back below */ }
if(trackedFiles===null){
  const skipDir=new Set([".git","node_modules","target","dist",".kilo"]);
  trackedFiles=[];
  const walk=(dir)=>{
    for(const e of fs.readdirSync(path.join(root,dir),{withFileTypes:true})){
      if(e.isDirectory()){ if(!skipDir.has(e.name)) walk(path.join(dir,e.name)); }
      else trackedFiles.push(path.join(dir,e.name).split(path.sep).join("/"));
    }
  };
  walk(".");
  console.log("Consumer-coverage check: git unavailable, using working-tree walk (untracked files included).");
}
const trackedText=new Map();
for(const f of trackedFiles){
  try { trackedText.set(f,fs.readFileSync(path.join(root,f),"utf8")); } catch { /* binary or unreadable */ }
}
// A basename shared by more than one tracked file identifies nothing on its own.
const basenameCounts=new Map();
for(const f of trackedFiles){
  const b=path.basename(f);
  basenameCounts.set(b,(basenameCounts.get(b)??0)+1);
}
// The gate's own source is a real consumer where it *reads* a file (it reads adapter-types.schema.json
// and native-to-mcf.registry.json, for instance), but its prose must not certify anything. This file is
// tracked and necessarily names the sources it audits — this very comment names one — so a source whose
// only real consumer had been deleted would otherwise still pass on the strength of the checker's own
// wording. Observed, not theorized: when the boundary fix above was first tested, this file's comment
// was the text credited with consuming `schemas/tauri-bridge-v1/payloads.schema.json`. Comments are
// therefore stripped from this file's text before it is used as evidence; its code still counts.
const SELF="tools/contracts/verify.mjs";
const stripComments=(s)=>s.replace(/\/\*[\s\S]*?\*\//g,"").split(/\r?\n/).map(l=>l.replace(/\/\/.*$/,"")).join("\n");
if(trackedText.has(SELF)) trackedText.set(SELF,stripComments(trackedText.get(SELF)));
for(const p of requiredRefs){
  const base=path.basename(p);
  const baseIsUnique=basenameCounts.get(base)===1;
  let consumer=null;
  for(const [f,text] of trackedText){
    if(f===p||f==="workspace.manifest.json") continue;
    if(namesFile(text,p)||(baseIsUnique&&namesFile(text,base))) { consumer=f; break; }
  }
  if(!consumer) fail(`Schema source has no consumer (no tracked file other than itself or the manifest names its path${baseIsUnique?` or its unique basename`:`; basename "${base}" is ambiguous so only an exact path counts`}): ${p}`);
}

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
// available (this repo has zero dependencies), so these guards are targeted line-based checks.
// Agent set: every canonical agent must appear as a top-level key, and no other agent may.
const capYamlPath="schemas/mcf-v2/conformance/adapter-capabilities.yaml";
const capYaml=fs.readFileSync(path.join(root,capYamlPath),"utf8");
for(const name of canonicalAgents){
  if(!new RegExp(`^  ${name}:`,"m").test(capYaml)) fail(`Conformance YAML missing agent block: ${name}`);
}
const capAgentBlocks=[...capYaml.matchAll(/^  ([A-Z][A-Z0-9_]*):/gm)].map(m=>m[1]);
for(const name of capAgentBlocks) if(!canonicalAgents.includes(name)) fail(`Conformance YAML declares non-canonical agent: ${name}`);

// Values, not just keys. The guard above proved insufficient: mutating a YAML VALUE (e.g. a
// transport id, acp_supported, or a required_environment entry) passed silently, and two of those
// mutations would re-open the session-upload hole. Extract each agent block by indentation and
// assert its scalar values agree with the authoritative transport contract.
const capBlock=(agent)=>{
  const lines=capYaml.split(/\r?\n/);
  const start=lines.findIndex(l=>l===`  ${agent}:`);
  if(start<0) return null;
  const out=[];
  for(let i=start+1;i<lines.length;i++){
    const l=lines[i];
    if(/^  [A-Z][A-Z0-9_]*:$/.test(l)) break;      // next agent block
    if(/^\S/.test(l)) break;                        // left the agents mapping entirely
    out.push(l);
  }
  return out.join("\n");
};
for(const name of canonicalAgents){
  const block=capBlock(name);
  const def=contract.agents[name];
  if(!block) fail(`Conformance YAML agent block unreadable: ${name}`);
  const scalar=(key)=>{
    const m=block.match(new RegExp(`^    ${key}:[ ]*(.*)$`,"m"));
    if(!m) return null;
    return m[1].trim().replace(/^["']|["']$/g,"");
  };
  const wantTransport=def.transport;
  const gotTransport=scalar("transport");
  if(gotTransport!==wantTransport) fail(`Conformance YAML ${name}.transport "${gotTransport}" != contract "${wantTransport}"`);
  // The contract states ACP support two ways: an explicit acp_supported boolean (HERMES_AGENT) or
  // the presence of an `acp` launch vector (KILO_CODE, OPEN_CODE). Accept either, and require the
  // YAML boolean to agree with whichever the contract uses.
  const wantAcpBool=def.acp_supported!==undefined ? def.acp_supported===true : Array.isArray(def.acp)&&def.acp.length>0;
  const gotAcp=scalar("acp_supported");
  if(gotAcp!==null){
    if(gotAcp!=="true" && gotAcp!=="false") fail(`Conformance YAML ${name}.acp_supported must be a boolean, got "${gotAcp}"`);
    if(gotAcp!==String(wantAcpBool)) fail(`Conformance YAML ${name}.acp_supported "${gotAcp}" != contract "${wantAcpBool}" (from ${def.acp_supported!==undefined?"acp_supported":"acp vector"})`);
  }
  // required_environment: every contract var must be present with the same value, and no extras.
  const envBlock=(block.match(/^    required_environment:\n((?:      .*\n?)*)/m)||[])[1];
  if(envBlock!==undefined){
    const got={};
    for(const m of envBlock.matchAll(/^      ([A-Z][A-Z0-9_]*):[ ]*(.*)$/gm)) got[m[1]]=m[2].trim().replace(/^["']|["']$/g,"");
    const want=def.required_environment||{};
    for(const [k,v] of Object.entries(want)){
      if(!(k in got)) fail(`Conformance YAML ${name}.required_environment missing ${k}`);
      else if(got[k]!==String(v)) fail(`Conformance YAML ${name}.required_environment ${k}="${got[k]}" != contract "${v}"`);
    }
    for(const k of Object.keys(got)) if(!(k in want)) fail(`Conformance YAML ${name}.required_environment has extra ${k} not in contract`);
  }
  // required_config: same shape/value as the contract's required_config.
  const cfgBlock=(block.match(/^    required_config:\n((?:      .*\n?)*)/m)||[])[1];
  if(cfgBlock!==undefined){
    const got={};
    for(const m of cfgBlock.matchAll(/^      ([A-Za-z0-9_.-]+):[ ]*(.*)$/gm)) got[m[1]]=m[2].trim().replace(/^["']|["']$/g,"");
    const want=def.required_config||{};
    for(const [k,v] of Object.entries(want)){
      if(!(k in got)) fail(`Conformance YAML ${name}.required_config missing ${k}`);
      else if(got[k]!==String(v)) fail(`Conformance YAML ${name}.required_config ${k}="${got[k]}" != contract "${v}"`);
    }
  }
  // The share setting is the session-upload kill switch; a YAML value of "auto" or "manual" here
  // while the contract says "disabled" is exactly the drift that would re-open the hole.
  if(def.required_config?.share!==undefined){
    const m=block.match(/^      share:[ ]*(.*)$/m);
    if(m){
      const v=m[1].trim().replace(/^["']|["']$/g,"");
      if(v!==String(def.required_config.share)) fail(`Conformance YAML ${name}.share "${v}" != contract "${def.required_config.share}"`);
    }
  }
  // The config-injection block is the permission kill switch. If the contract requires an
  // injection, the YAML must record the same channel and must not drop the default-deny base.
  if(def.required_config_injection!==undefined){
    const want=def.required_config_injection;
    // The injection's own keys sit at 6-space indent under config_injection:, one level deeper
    // than the agent-level scalars, so they need their own reader.
    // block has no trailing newline after its final line; add one so the lazy line repeat
    // can consume that last line too.
    const injBlock=(block+"\n").match(/^    config_injection:\n((?:(?:      .*)?\n)*)/m)?.[1];
    const injScalar=(key)=>{
      if(injBlock===undefined) return null;
      const m=injBlock.match(new RegExp(`^      ${key}:[ ]*(.*)$`,"m"));
      return m?m[1].trim().replace(/^["']|["']$/g,""):null;
    };
    const gotChannel=injScalar("channel");
    if(gotChannel===null) fail(`Conformance YAML ${name} omits config_injection.channel though the contract requires an injection`);
    else if(gotChannel!==want.channel) fail(`Conformance YAML ${name}.config_injection.channel "${gotChannel}" != contract "${want.channel}"`);
    const gotOrigin=injScalar("origin");
    if(gotOrigin===null) fail(`Conformance YAML ${name} omits config_injection.origin though the contract requires an injection`);
    else if(gotOrigin!==want.origin) fail(`Conformance YAML ${name}.config_injection.origin "${gotOrigin}" != contract "${want.origin}"`);
    const gotDefaultDeny=injScalar("default_deny");
    if(gotDefaultDeny===null) fail(`Conformance YAML ${name} omits config_injection.default_deny though the contract requires an injection`);
    else if(gotDefaultDeny!=="true") fail(`Conformance YAML ${name}.config_injection.default_deny must be true, got "${gotDefaultDeny}"`);
    // The wildcard base rule is what closes tools with no named permission key; it must exist.
    if(want.permission?.["*"]!==undefined && want.permission["*"]!=="deny") fail(`Contract ${name} config injection must default-deny, got "*": "${want.permission["*"]}"`);
    if(injBlock!==undefined){
      const listed=[...injBlock.matchAll(/^        - (.+)$/gm)].map(m=>m[1].trim());
      const wantTools=want.expected_enabled_tools||[];
      for(const t of wantTools) if(!listed.includes(t)) fail(`Conformance YAML ${name}.config_injection.expected_enabled_tools missing ${t}`);
      for(const t of listed) if(!wantTools.includes(t)) fail(`Conformance YAML ${name}.config_injection.expected_enabled_tools has extra ${t} not in contract`);
    }
  }
}

// native_kind enum and the native->MCF mapping keys must stay in lockstep.
const nativeKinds=read("schemas/agent-adapter-v1/native-event.schema.json").properties.native_kind.enum;
const nativeMap=read("schemas/agent-adapter-v1/native-to-mcf.registry.json").mappings;
for(const k of nativeKinds) if(!(k in nativeMap)) fail(`native_kind ${k} has no native-to-MCF mapping`);
for(const k of Object.keys(nativeMap)) if(!nativeKinds.includes(k)) fail(`native-to-MCF mapping ${k} is not a native_kind`);
for(const [k,v] of Object.entries(nativeMap)){
  if(v!==null && !messages.includes(v)) fail(`native-to-MCF mapping ${k} -> ${v} is not an MCF message type`);
}

// The native-event transport field must not be a free string: a normalized event could otherwise
// carry a transport that no probe can produce, and the agent_type/transport pairing would be
// unenforceable. Constrain it to the same enum probe-result uses, and require every transport the
// contract names to be in it.
const nativeTransport=read("schemas/agent-adapter-v1/native-event.schema.json").properties.transport;
const transportSet=nativeTransport.enum ?? (nativeTransport.const!==undefined?[nativeTransport.const]:null);
if(transportSet===null) fail("native-event.schema.json transport is unconstrained (no enum/const); a normalized event could carry an unknown transport");
for(const name of canonicalAgents){
  const t=contract.agents[name].transport;
  if(!transportSet.includes(t)) fail(`Contract transport ${t} (${name}) is not in native-event transport ${JSON.stringify(transportSet)}`);
}

// Structural validation of the transport contract. The contract is the machine-readable owner of
// DEC-024-critical adapter controls, but until now verify.mjs read only its `agents` keys and each
// agent's `transport` — so a required control field could be deleted and verification still passed.
// REQUIRED_CONTROLS pins, per agent, the fields whose absence would silently drop a gate. This is a
// per-agent map rather than a generic list because the controls differ: only OPEN_CODE is
// version-gated (its version_gate carries the fail-closed 2.x PWD-hazard branch), and only the
// fork-lineage agents carry the env hardening. Deleting a whole control object must fail here, not
// pass, so the map names the control even when nothing else in the file would reveal its absence.
const FORK_AGENTS=["KILO_CODE","OPEN_CODE"];
const REQUIRED_CONTROLS={
  HERMES_AGENT:["executable","launch","resume","version","transport","permission_enforcement","output_contract"],
  KILO_CODE:["executable","launch","resume","version","transport","permission_enforcement","output_contract","required_environment","required_config","required_config_injection"],
  OPEN_CODE:["executable","launch","resume","version","transport","permission_enforcement","output_contract","required_environment","required_config","required_config_injection","version_gate","required_environment_by_line","required_flags_by_line","determinism_flags_by_line","forbidden_commands_by_line","scope_hazards_2x","remote_forbidden_subcommands"],
};
for(const [name,def] of Object.entries(contract.agents)){
  const required=REQUIRED_CONTROLS[name];
  if(!required) fail(`Contract agent ${name} has no REQUIRED_CONTROLS entry; add one when the agent set changes`);
  for(const f of required){
    if(def[f]===undefined || (Array.isArray(def[f])&&def[f].length===0) || def[f]==="") fail(`Contract ${name} is missing required control: ${f}`);
  }
  // Any agent whose launch vector carries a flag that auto-approves must state how that is mediated.
  if(JSON.stringify(def.launch||[]).includes("--auto") && !def.permission_enforcement) fail(`Contract ${name} passes --auto but declares no permission_enforcement`);
  // The fork-lineage agents must carry the env hardening that closes the .claude leak and share paths.
  if(FORK_AGENTS.includes(name) && !def.required_environment) fail(`Contract ${name} is missing required_environment`);
  // A version-gated agent must carry a version_gate whose admitted_lines is non-empty. This is a
  // REQUIRED field for agents that declare admitted_version_lines or a versioned workspace_flag —
  // deleting the whole object (the fail-closed 2.x PWD-hazard control) must fail, not pass.
  const needsGate=def.admitted_version_lines!==undefined || def.version_gate!==undefined;
  if(needsGate){
    if(!def.version_gate) fail(`Contract ${name} declares versioned admission but has no version_gate object`);
    if(!Array.isArray(def.version_gate.admitted_lines) || def.version_gate.admitted_lines.length===0) fail(`Contract ${name}.version_gate has no admitted_lines`);
  }
  // Agents with no version gate must not silently gain one; and every agent must declare the
  // fail-closed posture when its version cannot be classified.
  if(def.version_gate && !def.version_gate.rationale) fail(`Contract ${name}.version_gate has no rationale`);
  // ACP support must agree with the launch vectors: an `acp` vector means supported; an explicit
  // acp_supported must not contradict it.
  const hasAcpVector=Array.isArray(def.acp)&&def.acp.length>0;
  if(def.acp_supported!==undefined && def.acp_supported!==hasAcpVector) fail(`Contract ${name}.acp_supported=${def.acp_supported} contradicts acp vector presence (${hasAcpVector})`);
  // A forbidden flag or command must never appear in the agent's own launch/resume/acp vectors —
  // the contract must not forbid a token it simultaneously passes.
  const vectors=[...(def.launch||[]),...(def.resume||[]),...(def.acp||[])].map(String);
  for(const tok of [...(def.remote_forbidden_flags||[]),...(def.remote_forbidden_commands||[])]){
    if(vectors.includes(tok)) fail(`Contract ${name} forbids "${tok}" but passes it in its own launch/resume/acp vectors`);
  }
  // A forbidden SUBcommand must be disjoint from the agent's vectors too, or the entry forbids a
  // path its own admitted vector walks. Checked on the first token of each subcommand ("auth export"
  // -> "auth") against the vector's first token, since a vector begins with its subcommand.
  if(vectors.length){
    const head=vectors[0];
    for(const sub of def.remote_forbidden_subcommands||[]){
      const [parent]=String(sub).split(/\s+/);
      if(parent===head) fail(`Contract ${name} forbids subcommand "${sub}" but its own launch vector begins with "${head}"`);
    }
  }
  // A permission_enforcement that names a forbidden flag in prose (e.g. OPEN_CODE's "do not pass
  // --auto") must also be absent from that agent's vectors.
  const m=String(def.permission_enforcement||"").match(/do not pass\s+(--[A-Za-z0-9-]+)/i);
  if(m && vectors.includes(m[1])) fail(`Contract ${name}.permission_enforcement says "do not pass ${m[1]}" but the vector passes it`);
  // --- Version-line scoping (OPEN_CODE). A vector, environment variable or determinism flag that is
  // valid only on one line must be marked as such, and a line that a control does not cover must
  // record its own mitigation. Without this, a 1.x-only vector silently reads as universal and an
  // adapter that keys on transport name rather than probed line fails at launch on the other line.
  if(def.version_gate){
    const lines=def.version_gate.admitted_lines||[];
    const unverified=def.version_gate.unverified_lines||[];
    // Every non-admitted line named in the gate must have an explicit admissibility statement, so a
    // line cannot be gated for the workspace flag alone while its launch/acp vectors go unscoped.
    if(unverified.length>0 && !def.version_gate.vector_admissibility_by_line) fail(`Contract ${name}.version_gate declares unverified lines ${JSON.stringify(unverified)} but no vector_admissibility_by_line; a 1.x-shaped launch/acp/determinism vector would read as universal`);
    if(def.version_gate.vector_admissibility_by_line){
      for(const line of [...lines,...unverified]){
        if(!def.version_gate.vector_admissibility_by_line[line]) fail(`Contract ${name}.version_gate.vector_admissibility_by_line has no entry for line "${line}"`);
      }
    }
    // A control that is line-scoped must be scoped for BOTH lines, so neither is left implicitly open.
    for(const field of ["required_environment_by_line","required_flags_by_line","determinism_flags_by_line","forbidden_commands_by_line"]){
      const scoped=def[field];
      if(!scoped) continue;
      for(const line of [...lines,...unverified]){
        if(!scoped[line]) fail(`Contract ${name}.${field} has no entry for line "${line}"`);
      }
    }
    // A union command list must say which line each name belongs to, or a name that exists on only one
    // line reads as a control for both. This is the staleness that produced a false "these commands are
    // missing" finding during the 2.x audit: the prose drifted from the array.
    if(unverified.length>0 && def.forbidden_commands_by_line){
      const attribution=Object.values(def.forbidden_commands_by_line).join(" ");
      for(const cmd of def.remote_forbidden_commands||[]){
        if(!new RegExp(`\\b${cmd.replace(/[.*+?^${}()|[\]\\]/g,"\\$&")}\\b`).test(attribution)) fail(`Contract ${name}.remote_forbidden_commands names "${cmd}" but no forbidden_commands_by_line entry attributes it to a line`);
      }
    }
    // A line-scoped environment/flag statement that mentions a variable or flag must not contradict
    // the base list: a token required on one line must not be forbidden on it.
    const forbidden=new Set([...(def.remote_forbidden_flags||[]),...(def.remote_forbidden_commands||[])]);
    for(const [line,stmt] of Object.entries(def.required_flags_by_line||{})){
      for(const tok of String(stmt).match(/--[A-Za-z0-9-]+/g)||[]){
        if(forbidden.has(tok)) fail(`Contract ${name}.required_flags_by_line["${line}"] names ${tok} as required but it is also in remote_forbidden_flags`);
      }
    }
    // Every forbidden flag must be scoped or justified for each line: a flag that exists on only one
    // line must not be presented as a control for both without a line note.
    const rationale=String(def.remote_forbidden_rationale||"");
    if(unverified.length>0 && !/2\.x/.test(rationale)) fail(`Contract ${name}.remote_forbidden_rationale does not address the 2.x line though the gate declares it`);
  }
  // The 2.x line's scope hazards are load-bearing (workspace/config/watcher/credential). If the gate
  // names 2.x, the entry must record how each is mitigated, or the gate is a version label only.
  if(def.version_gate?.unverified_lines?.includes("2.x") && !def.scope_hazards_2x) fail(`Contract ${name} gates 2.x but records no scope_hazards_2x; the 2.x workspace/config/watcher mitigations would be unstated`);
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
