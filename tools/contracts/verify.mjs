import fs from "node:fs";
import path from "node:path";
import {execFileSync} from "node:child_process";

const root=process.cwd();
const contentRead=new Set();
let gateCoverage={verified:0,canonical:0,unverified:0};
const read=(p)=>{contentRead.add(p);return JSON.parse(fs.readFileSync(path.join(root,p),"utf8"))};
const readText=(p)=>{contentRead.add(p);return fs.readFileSync(path.join(root,p),"utf8")};
// Parses without recording coverage. The readability sweep below opens every canonical JSON file, so if its
// reads counted as coverage evidence the coverage self-check could never fire: every declared file would look
// "read by a check" merely because it was parsed, and deleting a real invariant check would go unnoticed.
// Coverage has to mean "an invariant was enforced against these contents", so the sweep reads through this
// door instead.
const readUncounted=(p)=>JSON.parse(fs.readFileSync(path.join(root,p),"utf8"));
const exists=(p)=>fs.existsSync(path.join(root,p));
const fail=(m)=>{throw new Error(m)};

const mcf=read("schemas/mcf-v2/manifest.json");
for(const file of mcf.schemas ?? []) if(!exists("schemas/mcf-v2/"+file)) fail("Missing MCF schema: "+file);
for(const file of mcf.required_files ?? []) if(!exists("schemas/mcf-v2/"+file)) fail("Missing required MCF file: "+file);

const messages=read("schemas/mcf-v2/message-types.schema.json").enum;
const events=read("schemas/mcf-v2/event-types.schema.json").enum;
const registry=read("schemas/mcf-v2/registry.json");
const messagePayloads=read("schemas/mcf-v2/message-payloads.registry.json");
if(messages.length!==new Set(messages).size) fail("Duplicate MCF message type");
if(events.length!==new Set(events).size) fail("Duplicate MCF event type");
for(const name of messages) if(!messagePayloads.messages?.[name]) fail("No payload mapping for message: "+name);
for(const name of messages) if(!registry.message_priority?.[name]) fail("No priority mapping for message: "+name);

// --- Message-set cross-checks, the symmetric counterpart to the event checks below. DEC-033 claims
// contract verification enforces every message registry update a new message type requires; as written
// the gate read only two of them, and read them one-directionally — it checked that every enum member
// had a mapping, never that a list contained nothing the enum does not. Both directions are checked now.
// Three lists remain, not four: `registry.json:message_to_payload` was removed because it carried bare
// aliases with no declared meaning, and once normalized through the payload registry's own alias table
// it disagreed with `message-payloads.registry.json.messages` on 15 of 59 entries. Two registries for
// one mapping is the shadow-source pattern DEC-017 prohibits, so the payload registry is now the single
// authority and `refs` — which existed only to resolve those aliases — went with it.
const messageLists=[
  ["schemas/mcf-v2/registry.json","message_types",registry.message_types],
  ["schemas/mcf-v2/registry.json","message_priority",Object.keys(registry.message_priority??{})],
  ["schemas/mcf-v2/message-payloads.registry.json","messages",Object.keys(messagePayloads.messages??{})],
];
// --- Payload-value resolution. The list checks above compare key *sets*; they say nothing about the
// values, which is how `message_to_payload` could disagree with the payload registry on 15 entries and
// no check noticed. With one authority left, its values *are* the mapping, so each must resolve to a
// schema file that exists. Values are relative to the payload registry's own directory, so a
// `../<package>/<file>.schema.json` reference legitimately resolves outside schemas/mcf-v2/.
for(const [name,ref] of Object.entries(messagePayloads.messages??{})){
  if(typeof ref!=="string"||ref.length===0) fail(`Payload mapping for ${name} is not a non-empty string`);
  if(!exists(path.join("schemas/mcf-v2",ref))) fail(`Payload mapping for ${name} points at a missing schema: ${ref}`);
}
// --- message_to_event. This map records which event a message causes, and was read by no tool:
// `git grep` finds only its own definition in registry.json. It is currently correct - every key is a
// message type and every value a canonical event - but nothing enforced that, so it would have drifted
// the way message_to_payload did. It is incomplete by design (13 of 59 messages), so this checks
// membership only, never coverage: a message that causes no event of its own is legitimately absent.
const messageToEvent=registry.message_to_event;
if(!messageToEvent||typeof messageToEvent!=="object") fail("registry.json has no message_to_event block");
const messageSet=new Set(messages), eventSet=new Set(events);
for(const [msg,ev] of Object.entries(messageToEvent)){
  if(!messageSet.has(msg)) fail(`message_to_event names "${msg}", which is not a message type in message-types.schema.json`);
  if(!eventSet.has(ev)) fail(`message_to_event maps ${msg} to "${ev}", which is not a canonical event in event-types.schema.json`);
}
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

// --- Registry identity/version agreement. MCF-V2-MACHINE-READABLE-CONTRACT.md:148 states that
// message-types.schema.json, event-types.schema.json and transition-types.json "must reference the
// same registry identity/version". That sentence was unbacked in three ways: the two .schema.json
// files named by it carried no version field at all; the two files that did carry `registry_version`
// disagreed (registry.json 2.0.0 vs transition-types.json 2.1.0, and they had never matched since
// transition-types.json first carried one); and no "registry identity" concept existed anywhere.
// No tool read any version field, so nothing could notice. manifest.json:version is now the single
// package version, every registry file must carry `registry_version` equal to it, and the protocol
// namespace must agree across the files that declare one. Runtime validators are required to report
// the registry version in errors and evidence (MCF-V2-IMPLEMENTATION-DESIGN.md:95), so this is the
// value they will read; if the files can disagree, that report is meaningless.
const packageVersion=mcf.version;
if(!packageVersion) fail("schemas/mcf-v2/manifest.json has no version; it is the canonical package version");
const versionedRegistryFiles=[
  "schemas/mcf-v2/registry.json",
  "schemas/mcf-v2/transition-types.json",
  "schemas/mcf-v2/event-types.schema.json",
  "schemas/mcf-v2/message-types.schema.json",
  "schemas/mcf-v2/event-to-ui.registry.json",
  "schemas/mcf-v2/event-payloads.registry.json",
  "schemas/mcf-v2/message-payloads.registry.json",
];
for(const file of versionedRegistryFiles){
  const got=read(file).registry_version;
  if(got===undefined) fail(`Registry file declares no registry_version: ${file} (must equal manifest.json version "${packageVersion}")`);
  if(got!==packageVersion) fail(`registry_version disagreement: ${file}="${got}" vs manifest.json="${packageVersion}"`);
}
// The protocol namespace is the other half of "registry identity". It is not carried by every file
// (only registry.json and manifest.json declare it), so this checks agreement where it is declared
// rather than requiring the field everywhere.
const protocolNamespaces=[["schemas/mcf-v2/registry.json",registry.protocol],["schemas/mcf-v2/manifest.json",mcf.protocol]];
for(const [file,ns] of protocolNamespaces){
  if(ns===undefined) fail(`Registry file declares no protocol namespace: ${file}`);
  if(ns!==mcf.protocol) fail(`protocol namespace disagreement: ${file}="${ns}" vs manifest.json="${mcf.protocol}"`);
}

const transition=read("schemas/mcf-v2/transition-types.json");
// --- Adjacency, off-spine declaration, and the machine spine (DEC-038, DEC-042).
// Adjacency is checked over each machine's declared `spine`, not over its whole `states` set.
// `states` is the set of legal states and legitimately includes terminal/branch states that are not
// on the ordered path; `spine` is that ordered path. Requiring states[i]->states[i+1] across the
// whole set manufactures transitions out of terminal states - message_delivery.PROCESSED->RETRYING
// and task.COMPLETED->BLOCKED were mandatory edges, not accidental ones.
// `spine` is now REQUIRED on every machine. It previously fell back to `states`, which is the same
// silent-default defect this series removes: a machine that forgot to declare a spine was silently
// treated as having one. A one-element spine is the explicit way to say "this machine has no spine
// edges" (context: the path is CURRENT alone, with SUPERSEDED and INVALIDATED as terminal branches).
// Every off-spine edge must then be declared - in `branches` (blessed) or `unreviewed_branches`
// (legal but not yet reviewed). A new off-spine edge in neither list fails, so the forbid rule is live
// from now on even though nine pre-existing edges are still unreviewed rather than guessed at.
// `unreviewed_branches` is deliberately the weaker claim: "we have not reviewed this" rather than
// "this is undecided". It is given teeth by requiring every entry to be ADVANCE-driven - a branch
// driven by a specific command is decidable, so it belongs in `branches`, not here.
const offSpineProblems=[];
for(const [machine,def] of Object.entries(transition.machines)){
  const ids=new Set((transition.transitions?.[machine]??[]).map(t=>t.transition_id));
  if(!Array.isArray(def.spine)) { offSpineProblems.push("Machine "+machine+" declares no spine; every machine must declare one explicitly (a one-element spine means no spine edges)"); continue; }
  const stray=def.spine.filter(s=>!def.states.includes(s));
  if(stray.length) { offSpineProblems.push("Machine "+machine+" spine names state(s) absent from states: "+stray.join(", ")); continue; }
  for(let i=0;i<def.spine.length-1;i++) {
    const id=`${machine}.${def.spine[i]}->${def.spine[i+1]}`;
    if(!ids.has(id)) offSpineProblems.push("Missing adjacent transition on "+machine+" spine: "+id);
  }
  const onSpine=new Set();
  for(let i=0;i<def.spine.length-1;i++) onSpine.add(def.spine[i]+"->"+def.spine[i+1]);
  const edges=[...new Set((transition.transitions?.[machine]??[]).map(t=>t.transition_id.slice(machine.length+1)))];
  const declared=new Set([...(def.branches??[]),...(def.unreviewed_branches??[])]);
  const adv="ADVANCE_"+machine.toUpperCase();
  for(const e of edges) if(!onSpine.has(e)&&!declared.has(e)) offSpineProblems.push("Undeclared off-spine edge "+machine+"."+e+" - add it to branches (blessed) or unreviewed_branches (legal but not yet reviewed)");
  for(const e of declared) if(!edges.includes(e)) offSpineProblems.push("Declared branch "+machine+"."+e+" has no transition record");
  for(const e of (def.unreviewed_branches??[])){
    const recs=(transition.transitions?.[machine]??[]).filter(t=>t.transition_id.slice(machine.length+1)===e);
    if(recs.length&&!recs.every(t=>t.command===adv)) offSpineProblems.push("Unreviewed branch "+machine+"."+e+" is driven by a specific command, so it is decidable and belongs in branches, not unreviewed_branches");
  }
}
if(offSpineProblems.length) fail(offSpineProblems.length+" spine/off-spine problem(s):\n  - "+offSpineProblems.join("\n  - "));

// --- Transition event-type validity. registry.json:transition_event_rules states the contract
// this section enforces: "Every domain state transition has exactly one authoritative owner and
// exactly one canonical event type." Nothing read a transition record's `event_type` at all, so
// both halves were violated and undetected. 32 records named an event that is not an MCF event
// (AGENT_SESSION_CHANGED, TASK_CHANGED, CONTEXT_CHANGED, DEAD_LETTER — all four are Tauri UI event
// names, which is the same two-vocabulary confusion that DEC-035's ADMISSION_RECORDED drift came
// from), and a further 16 named a real MCF event belonging to a different edge of the same machine
// (e.g. lease.ACTIVE->RENEWING carried LEASE_REQUESTED, the event for entering ACTIVE). A record's
// event must be a member of the canonical enum, and every record for one transition_id must agree,
// so an edge cannot carry one event on its specific-command record and another on its ADVANCE_ spine
// record. `emitted_events` is checked with it: a transition that mutates state to X but emits Y
// describes two different transitions, and 157/157 records emitted exactly [event_type] before this
// was enforced, so the equality is the existing convention rather than a new one.
const eventEnum=new Set(events);
const transitionEventById=new Map();
for(const [machine,list] of Object.entries(transition.transitions??{})){
  for(const t of list){
    if(!eventEnum.has(t.event_type)) fail(`Transition ${t.transition_id} declares event_type "${t.event_type}", which is not an MCF event in event-types.schema.json`);
    if(JSON.stringify(t.emitted_events)!==JSON.stringify([t.event_type])) fail(`Transition ${t.transition_id} emits ${JSON.stringify(t.emitted_events)} but mutates on event_type "${t.event_type}"; emitted_events must be exactly [event_type]`);
    if(transitionEventById.has(t.transition_id)&&transitionEventById.get(t.transition_id)!==t.event_type) fail(`Transition ${t.transition_id} has two canonical events: "${transitionEventById.get(t.transition_id)}" and "${t.event_type}"`);
    transitionEventById.set(t.transition_id,t.event_type);
  }
}

// --- Transition command registry. 58 distinct commands drove the 157 transition records and were
// named nowhere outside them, so a command had no declared owner and no way to be found except by
// reading every record. registry.json:transition_commands now registers each one with its machine,
// kind, owning service and crate. The set must match the records exactly in both directions: an
// unregistered command is an undeclared state-machine driver, and a registered command nothing uses
// is a phantom driver that a reader would take for a real one. The machine and crate are derived
// facts and are checked against the record that uses the command and against transition.machines,
// so the registration cannot quietly disagree with the transition registry it describes.
const commandRegistry=registry.transition_commands?.commands;
if(!commandRegistry||typeof commandRegistry!=="object") fail("registry.json has no transition_commands.commands block");
const declaredCommands=new Map();
for(const [machine,list] of Object.entries(transition.transitions??{})){
  for(const t of list){
    const prev=declaredCommands.get(t.command);
    if(prev&&prev.machine!==machine) fail(`Command ${t.command} is used by two machines: ${prev.machine} and ${machine}`);
    declaredCommands.set(t.command,{machine,transitionId:t.transition_id});
  }
}
for(const [c,rec] of Object.entries(commandRegistry)){
  if(!declaredCommands.has(c)) fail(`Registered transition command ${c} is used by no transition record`);
  const {machine,transitionId}=declaredCommands.get(c);
  if(rec.machine!==machine) fail(`Transition command ${c} is registered under machine "${rec.machine}" but used by ${machine} (${transitionId})`);
  const owner=transition.machines?.[machine]?.owner;
  if(rec.crate!==owner) fail(`Transition command ${c} is registered with crate "${rec.crate}" but machine ${machine} is owned by "${owner}"`);
}
for(const [c,{transitionId}] of declaredCommands){
  if(!(c in commandRegistry)) fail(`Transition ${transitionId} uses command ${c}, which is not registered in registry.json:transition_commands`);
}
// The ADVANCE_<MACHINE> convention drives a machine's adjacent spine, so it is required exactly when
// there is a spine to drive, and forbidden when there is not. Both directions are checked: a
// requirement alone forces a command onto a machine with no subject, and an exemption alone lets a
// machine with no spine keep a dangling advance command (or a used one, which the global
// "registered but unused" check below cannot see). The condition is derived from the declared spine
// rather than carried as a flag, so it cannot drift from the data.
for(const [machine,def] of Object.entries(transition.machines??{})){
  const want="ADVANCE_"+machine.toUpperCase();
  const spine=def.spine??[];
  if(spine.length<2){
    if(want in commandRegistry) fail(`Machine ${machine} declares a ${spine.length}-state spine, so it has no spine edge to advance, and ${want} must not be registered`);
    continue;
  }
  if(!(want in commandRegistry)) fail(`Machine ${machine} declares a ${spine.length}-state spine but has no ${want} command registered`);
  if(commandRegistry[want].kind!=="advance") fail(`Command ${want} must be registered with kind "advance"`);
  if(commandRegistry[want].machine!==machine) fail(`Command ${want} is registered under machine "${commandRegistry[want].machine}", expected "${machine}"`);
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

// --- The bridge gate is two-way (DEC-053).
// Every bridge check above reads the contract and nothing else, so the gate could prove the contract was
// self-consistent while being blind to both failures that actually existed: a handler registered under a name
// the contract does not declare (`recovery_status`, where the contract declares `get_recovery_status`), and an
// operation the contract declares that nothing implements. A gate that reads one side of a two-sided agreement
// cannot detect a disagreement, so both sides are read here.
//
// The two directions are deliberately NOT symmetric, and the asymmetry is the design rather than a softening:
//
//   implementation -> contract   is a FAILURE. A registered handler or a transport call that names an
//                                operation the contract does not declare is drift with no defence: the
//                                contract is the authority (AGENTS.md section 5), so the code is wrong.
//   contract -> implementation   is REPORTED. The contract deliberately leads implementation - it declares 59
//                                operations and 4 exist - so "declared and unimplemented" cannot be a failure
//                                without either deleting 55 declarations or introducing a second registry of
//                                "pending" operations, which would be a competing source of truth (DEC-017).
//                                It is reported with its count and names on every run so the gap stays visible
//                                instead of being silently absorbed by a green gate.
//
// A call site whose operation name is not a string literal is a FAILURE rather than a skip: an unresolvable
// name is precisely the case the check cannot cover, and ignoring it would let the drift this block exists to
// catch walk through the one door the check cannot see. Comments are stripped before scanning, so a name
// mentioned in prose is not mistaken for a call site.

// Removes comments while preserving string literals. A regex-only stripper gets one of the two wrong: it either
// leaves a commented-out call site looking live, or eats a `//` inside a string. State is tracked explicitly.
// Named distinctly from the simpler `stripComments` used later for this gate's own text, which must keep its
// existing behaviour. Regex literals are not modelled; none of the scanned files (Rust, or the frontend's
// non-generated TypeScript) contains one, and a desynchronized scan fails loudly rather than passing quietly.
const stripCommentsForScan=(src)=>{
  let out="",state="code",i=0;
  while(i<src.length){
    const c=src[i],n=src[i+1];
    if(state==="code"){
      if(c==="/"&&n==="/"){state="line";i+=2;continue;}
      if(c==="/"&&n==="*"){state="block";i+=2;continue;}
      if(c==="'"||c==='"'||c==="`"){state=c;out+=c;i++;continue;}
      out+=c;i++;continue;
    }
    if(state==="line"){ if(c==="\n"){state="code";out+=c;} i++; continue; }
    if(state==="block"){ if(c==="*"&&n==="/"){state="code";i+=2;continue;} i++; continue; }
    if(c==="\\"){out+=c+(n??"");i+=2;continue;}
    if(c===state){state="code";out+=c;i++;continue;}
    out+=c;i++;
  }
  return out;
};

const walkFiles=(dir)=>{
  const out=[];
  const visit=(rel)=>{
    for(const entry of fs.readdirSync(path.join(root,rel),{withFileTypes:true})){
      const child=rel+"/"+entry.name;
      if(entry.isDirectory()) visit(child); else out.push(child);
    }
  };
  visit(dir);
  return out;
};

const BRIDGE_RUST="apps/desktop/src-tauri/src/main.rs";
const BRIDGE_BOUNDARY="apps/desktop/src/intake/bridge.ts";
const declaredOps=new Map();
for(const c of bridge.properties.command.enum) declaredOps.set(c,"command");
for(const q of bridge.properties.query.enum) declaredOps.set(q,"query");

const bridgeProblems=[];

// --- Rust side: which handlers are registered, and which functions are actually commands.
const rustCode=stripCommentsForScan(readText(BRIDGE_RUST));
// The macro name also appears inside string literals in this file - the shell's own tests name it in their
// assertion messages - and `stripCommentsForScan` deliberately preserves string literals, so a single
// first-match lookup can read a phantom list out of quoted prose. Every candidate is therefore collected and
// the first whose contents are all handler names is used; if none is, the check fails closed. Without this, a
// registration list the gate could not parse could be replaced by one quoted in a string, and the gate would
// then validate the whole bridge against a list that no `generate_handler!` call contains.
const handlerCandidates=[...rustCode.matchAll(/generate_handler!\s*\[([\s\S]*?)\]/g)];
const isHandlerList=(body)=>{
  const names=body.split(",").map(s=>s.trim()).filter(Boolean);
  return names.length>0&&names.every(n=>/^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*$/.test(n));
};
const handlerList=handlerCandidates.find(m=>isHandlerList(m[1]));
if(!handlerList) fail(
  `Could not find a generate_handler![...] list in ${BRIDGE_RUST}.\n`+
  `The bridge gate reads this file to compare registered handlers against the contract, so a registration list `+
  `it cannot parse is a failure rather than a skip: a handler nothing can enumerate is a handler nothing can check.`
);
const registered=handlerList[1].split(",").map(s=>s.trim()).filter(Boolean).map(s=>s.split("::").pop());
for(const n of registered) if(!/^[A-Za-z_][A-Za-z0-9_]*$/.test(n)) fail(
  `${BRIDGE_RUST}: could not parse "${n}" in generate_handler![...] as a handler name.`
);
// Fail closed on an attribute whose function name cannot be read: such a function would be registered or not
// with nothing noticing, which is the state this check exists to end.
//
// The attribute may carry arguments - `#[tauri::command(rename_all = "snake_case")]` - so its argument list is
// captured rather than assumed absent, and a bare `#[tauri::command]` is still matched with an empty one. An
// earlier version of this check hard-coded the bare form and would have reported every correctly-annotated
// command as missing, which is a check that fails on the shape of the annotation rather than on the bridge.
const commandAttrCount=(rustCode.match(/#\[tauri::command\s*(?:\([^)]*\))?\s*\]/g)??[]).length;
const commandFns=[...rustCode.matchAll(
  /#\[tauri::command\s*(?:\(([^)]*)\))?\s*\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)/g
)].map(m=>({name:m[2],args:m[1]??"",params:m[3]}));
if(commandFns.length!==commandAttrCount) fail(
  `${BRIDGE_RUST}: ${commandAttrCount} #[tauri::command] attribute(s) but only ${commandFns.length} resolved to a function name.\n`+
  `An attribute whose function name cannot be read is unchecked, so this is a failure rather than a skip.`
);
const commandNames=commandFns.map(c=>c.name);
for(const n of registered) if(!commandNames.includes(n)) bridgeProblems.push(
  `${BRIDGE_RUST} registers ${n} in generate_handler![...] but defines no #[tauri::command] fn ${n}`
);
for(const n of commandNames) if(!registered.includes(n)) bridgeProblems.push(
  `${BRIDGE_RUST} defines #[tauri::command] fn ${n} but never registers it in generate_handler![...], so it is unreachable from the frontend`
);
for(const n of registered) if(!declaredOps.has(n)) bridgeProblems.push(
  `${BRIDGE_RUST} registers handler ${n}, which the bridge contract declares as neither a command nor a query`
);

// --- Frontend side: what the Control Room actually asks Rust for. Generated files are excluded because they
// are derived from the contract rather than from an intent to call something, and tests are excluded because a
// test's fake transport legitimately names whatever it is exercising.
const frontendFiles=walkFiles("apps/desktop/src")
  .filter(f=>/\.(ts|tsx)$/.test(f))
  .filter(f=>!f.startsWith("apps/desktop/src/generated/"))
  .filter(f=>!/\.test\.(ts|tsx)$/.test(f));
const transportCalls=[];
for(const file of frontendFiles){
  const code=stripCommentsForScan(readText(file));
  for(const m of code.matchAll(/(?<![A-Za-z0-9_$])transport\s*\(/g)){
    const literal=/^\s*(["'])([^"']*)\1/.exec(code.slice(m.index+m[0].length));
    if(!literal){
      bridgeProblems.push(`${file} calls transport(...) with an operation name that is not a string literal, so the name cannot be checked against the contract`);
      continue;
    }
    transportCalls.push([file,literal[2]]);
  }
  // `invoke` is the raw Tauri boundary. It is permitted in exactly one file, because that file is the single
  // controlled transport boundary (AGENTS.md section 11); anywhere else it is a second, unchecked door.
  for(const m of code.matchAll(/(?<![A-Za-z0-9_$.])invoke\s*\(/g)){
    if(file===BRIDGE_BOUNDARY) continue;
    const literal=/^\s*(["'])([^"']*)\1/.exec(code.slice(m.index+m[0].length));
    bridgeProblems.push(
      `${file} calls invoke(...) directly instead of going through the single transport boundary ${BRIDGE_BOUNDARY}`+
      (literal?` (it names ${literal[2]})`:"")
    );
  }
}
for(const [file,name] of transportCalls){
  if(!declaredOps.has(name)) bridgeProblems.push(
    `${file} calls transport("${name}"), which the bridge contract declares as neither a command nor a query`
  );
  else if(!registered.includes(name)) bridgeProblems.push(
    `${file} calls transport("${name}") but ${BRIDGE_RUST} registers no handler of that name, so the declared ${declaredOps.get(name)} cannot succeed`
  );
}
if(bridgeProblems.length) fail(
  `The Tauri bridge disagrees with its own contract (DEC-053):\n  - ${bridgeProblems.join("\n  - ")}\n`+
  `The contract is the authority (AGENTS.md section 5): declare the operation, or register it under its declared name.`
);

// The reported direction. Kept as a value so the summary can state it rather than leaving it implied.
const bridgeImplemented=[...declaredOps.keys()].filter(n=>registered.includes(n));
const bridgeUnimplemented=[...declaredOps.keys()].filter(n=>!registered.includes(n));

// --- Event emitters (DEC-040). registry.json:event_ownership is keyed by event *category*, not by
// event type, so it has no per-event key and cannot answer "what emits this event?". event_emitters
// declares one emitter per canonical event: {kind:"transition", machines:[...]} or
// {kind:"service", service, crate} - service null where the crate owns the concern directly.
// Three checks: coverage (every event declares an emitter, no emitter names a non-event); a
// transition emitter must actually be emitted by one of its declared machines; a service emitter
// must name a real application service and the crate that owns it, or a null service with a real
// crate.
// Violations are collected and reported together rather than failing on the first. This check is
// expected to be red on a known set while the per-machine event repairs land, and a fail-fast report
// would hide all but one of them - the count is the useful signal.
// Known limit, stated so it is not overclaimed: the transition check verifies emission BY MACHINE,
// not by target state. An event attached to the wrong edge of the right machine still passes; that
// is recorded as a DEC-040 limitation rather than implied away.
const emitters=registry.event_emitters;
if(!emitters||typeof emitters!=="object") fail("registry.json has no event_emitters block");
const emittedByMachine={};
for(const [machine,list] of Object.entries(transition.transitions??{})){
  for(const t of list) (emittedByMachine[t.event_type]??=new Set()).add(machine);
}
const serviceCrate=workspace.application_services??{};
const crateNames=Object.keys(workspace.crates??{}).map(c=>"crates/"+c);
const emitterProblems=[];
for(const e of events){
  const d=emitters[e];
  if(!d){ emitterProblems.push("Event "+e+" declares no emitter"); continue; }
  if(d.kind==="transition"){
    if(!Array.isArray(d.machines)||d.machines.length===0){ emitterProblems.push("Event "+e+" is kind transition but declares no machines"); continue; }
    const unknown=d.machines.filter(m=>!transition.machines?.[m]);
    if(unknown.length){ emitterProblems.push("Event "+e+" declares unknown machine(s): "+unknown.join(", ")); continue; }
    if(!d.machines.some(m=>emittedByMachine[e]?.has(m))) emitterProblems.push("Event "+e+" is declared a transition emitter for ["+d.machines.join(", ")+"] but no transition in that machine set emits it");
  } else if(d.kind==="service"){
    if(d.service!==null){
      if(!(d.service in serviceCrate)){ emitterProblems.push("Event "+e+" declares unknown service "+JSON.stringify(d.service)); continue; }
      const want="crates/"+serviceCrate[d.service];
      if(d.crate!==want){ emitterProblems.push("Event "+e+" declares crate "+JSON.stringify(d.crate)+" but service "+JSON.stringify(d.service)+" is owned by "+JSON.stringify(want)); continue; }
    }
    if(!crateNames.includes(d.crate)) emitterProblems.push("Event "+e+" declares unknown crate "+JSON.stringify(d.crate));
  } else {
    emitterProblems.push("Event "+e+" declares unknown emitter kind "+JSON.stringify(d.kind));
  }
}
for(const e of Object.keys(emitters)) if(!events.includes(e)) emitterProblems.push("event_emitters names "+JSON.stringify(e)+", which is not a canonical event");
if(emitterProblems.length) fail(emitterProblems.length+" event-emitter problem(s):\n  - "+emitterProblems.join("\n  - "));

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
  HERMES_AGENT:["lineage_group","executable","launch","resume","version","transport","permission_enforcement","output_contract"],
  KILO_CODE:["lineage_group","executable","launch","resume","version","transport","permission_enforcement","output_contract","required_environment","required_config","required_config_injection"],
  OPEN_CODE:["lineage_group","executable","launch","resume","version","transport","permission_enforcement","output_contract","required_environment","required_config","required_config_injection","version_gate","required_environment_by_line","required_flags_by_line","determinism_flags_by_line","forbidden_commands_by_line","scope_hazards_2x","remote_forbidden_subcommands"],
};
// --- Lineage partition. Council corroboration counts distinct LINEAGE GROUPS rather than agents, because Kilo
// Code CLI is a fork of OpenCode CLI and their agreement is not independent evidence (ARCHITECTURE.md section
// 2, COUNCIL-ENGINE.md Purpose). That fact lived only in prose, so a corroboration rule could not be computed
// from the contract and a hand-written table in code would have been the drift this repository keeps removing.
// `lineage_group` is therefore a required adapter control, and the partition is pinned: the fork pair share a
// group and Hermes stands alone, so the structure the prose describes is the structure the contract carries.
// Pinning the group NAMES also means a rename cannot silently split or merge a lineage.
const CANONICAL_LINEAGES={HERMES_AGENT:"HERMES",KILO_CODE:"OPENCODE_FORK",OPEN_CODE:"OPENCODE_FORK"};
for(const [name,want] of Object.entries(CANONICAL_LINEAGES)){
  const got=contract.agents[name]?.lineage_group;
  if(got!==want) fail(`Contract ${name}.lineage_group is ${JSON.stringify(got)}, expected ${JSON.stringify(want)} from the documented lineage (ARCHITECTURE.md section 2: Kilo Code CLI is a fork of OpenCode CLI)`);
}
const lineageGroups=new Set(Object.values(CANONICAL_LINEAGES));
if(lineageGroups.size<2) fail(`The agent set declares ${lineageGroups.size} lineage group(s); council corroboration requires at least 2, so either the contract or the CORROBORATION rule is wrong`);
if(CANONICAL_LINEAGES.KILO_CODE===CANONICAL_LINEAGES.HERMES_AGENT) fail(`The fork pair and the independent agent must not share a lineage group; corroboration between them would be counted as independent`);
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

// --- DATA-MODEL core entities vs the canonical SQLite schema.
// docs/DATA-MODEL.md owns the entity inventory; schemas/sqlite-v1/schema.sql owns persistence. An entity
// documented as durable with no table is a contract divergence: the documentation asserts persistence the
// schema does not implement, so an agent can read one and implement the other, or invent a third.
// Nothing read schema.sql before this check, so the divergence was invisible to a gate that was otherwise
// fully green - the enforcement this series built was blind to exactly the layer where an implementation
// agent looks first.
//
// The entity list is read from the document's own machine-parseable "Core entities" block rather than
// inferred from prose. A regex over prose cannot decide whether a CamelCase term is an entity, a value
// object or a column, and guessing produces false positives that get dismissed, which is worse than no
// check at all. If that block cannot be parsed the check fails loudly rather than passing on nothing.
const dataModel=readText("docs/DATA-MODEL.md");
const coreBlock=dataModel.split(/^##\s+Core entities\s*$/m)[1]?.split(/^##\s+/m)[0] ?? "";
const coreEntities=coreBlock.split(/\r?\n/).map(l=>l.trim()).filter(l=>/^[A-Z][A-Za-z]*$/.test(l));
if(coreEntities.length<10) fail(`DATA-MODEL.md Core entities block parsed to ${coreEntities.length} entities; the document structure changed and this check can no longer read it`);
const sqlText=readText("schemas/sqlite-v1/schema.sql");
// Extracted with `--` line comments removed. The extraction below is a regex over raw text, so a comment that
// merely MENTIONS `CREATE TABLE IF NOT EXISTS` was read as a table definition: a header comment beginning
// "applies this file with CREATE TABLE IF NOT EXISTS on every open" produced a phantom table named `on`, and
// the check then demanded it be documented. Comments are not schema, so they are stripped before any
// structural extraction. Reproduced before this fix and re-verified after it.
const sqlCode=sqlText.replace(/--[^\n]*/g,"");
const sqlTables=new Set([...sqlCode.matchAll(/CREATE TABLE IF NOT EXISTS\s+(\w+)/g)].map(m=>m[1]));
const snake=(n)=>n.replace(/([a-z0-9])([A-Z])/g,"$1_$2").toLowerCase();
// English pluralisation is irregular and a naive rule produces false positives, which is the same reason
// prose regexes are not trusted above. One general rule plus an explicit override for words it cannot
// know: a new entity with an irregular plural must be declared here rather than silently mismatched.
const irregularPlurals=new Map([["project_epoch","project_epochs"]]);
const plural=(s)=>{
  if(irregularPlurals.has(s)) return irregularPlurals.get(s);
  if(/is$/.test(s)) return s.slice(0,-2)+"es";
  if(/(?:s|x|z|ch|sh)$/.test(s)) return s+"es";
  if(/[^aeiou]y$/.test(s)) return s.slice(0,-1)+"ies";
  return s+"s";
};
// An entity may legitimately be a value object or an alias rather than its own table. Declaring that with
// a reason is what makes this a contract rather than a guess, and it is why adding a table is not the only
// way to satisfy the check. Both entries below are justified by an owning machine, not by convenience.
const entityStorage=new Map([
  ["ProjectStatus",{storage:"projects.status",why:"value object, not a durable entity; project status is a column on projects"}],
  ["Checkpoint",{storage:"workspace_checkpoints",why:"alias; the documented checkpoint is the workspace checkpoint, owned by the workspace machine"}],
]);
const entityProblems=[];
for(const e of coreEntities){
  const declared=entityStorage.get(e);
  if(declared){
    const target=declared.storage.split(".")[0];
    if(!sqlTables.has(target)) entityProblems.push(`Entity ${e} is declared as stored in ${declared.storage}, but no table ${target} exists in schema.sql`);
    continue;
  }
  const base=snake(e);
  if(sqlTables.has(base)||sqlTables.has(plural(base))) continue;
  entityProblems.push(`Entity ${e} is documented as a core entity in DATA-MODEL.md but has no table in schema.sql (tried ${base}, ${plural(base)}). Add the table, or declare its storage in verify.mjs with a reason.`);
}
if(entityProblems.length) fail(entityProblems.length+" DATA-MODEL entity/persistence divergence(s):\n  - "+entityProblems.join("\n  - "));

// --- SQLite schema referential integrity. schema.sql is the durable local source of truth, and until the
// entity check above nothing in this file read it at all. A foreign key naming a table that does not exist
// is accepted by SQLite until the moment a row is written, so the divergence surfaces at runtime rather
// than at contract time.
const sqlTablesArr=[...sqlTables];
const danglingFk=[];
for(const m of sqlText.matchAll(/REFERENCES\s+(\w+)\s*\(/g)){
  if(!sqlTables.has(m[1])) danglingFk.push(m[1]);
}
if(danglingFk.length) fail(`schema.sql declares foreign keys to non-existent table(s): ${[...new Set(danglingFk)].join(", ")}`);

// SQLite validates the referenced table name, but a malformed CREATE TABLE can still carry a FOREIGN KEY
// whose local column was never declared. Require every local and referenced column to exist in the corresponding
// table. This is structural validation of the canonical SQL, not a second schema authority.
const sqliteFkLocalProblems=[];
const sqliteTableColumns=new Map();
for(const tableMatch of sqlText.matchAll(/CREATE TABLE IF NOT EXISTS\s+(\w+)\s*\(([\s\S]*?)\n\);/g)){
  const [,tableName,body]=tableMatch;
  const columns=new Set();
  for(const line of body.split(/\r?\n/)){
    const trimmed=line.trim();
    if(!trimmed || /^(?:PRIMARY KEY|FOREIGN KEY|UNIQUE|CHECK|CONSTRAINT)\b/i.test(trimmed)) continue;
    const column=trimmed.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+/);
    if(column) columns.add(column[1]);
  }
  sqliteTableColumns.set(tableName,columns);
  for(const fk of body.matchAll(/FOREIGN KEY\s*\(([^)]+)\)\s*REFERENCES\s+(\w+)\s*\(([^)]+)\)/gi)){
    const locals=fk[1].split(",").map(v=>v.trim()).filter(Boolean);
    const targetTable=fk[2];
    const targets=fk[3].split(",").map(v=>v.trim()).filter(Boolean);
    if(locals.length!==targets.length){
      sqliteFkLocalProblems.push(`table ${tableName}: FOREIGN KEY(${fk[1]}) references ${targetTable}(${fk[3]}) with mismatched local/target column counts`);
      continue;
    }
    for(let n=0;n<locals.length;n++){
      if(!columns.has(locals[n])){
        sqliteFkLocalProblems.push(`table ${tableName}: FOREIGN KEY local column "${locals[n]}" is not declared in the table`);
      }
      const targetColumns=sqliteTableColumns.get(targetTable);
      if(targetColumns && !targetColumns.has(targets[n])){
        sqliteFkLocalProblems.push(`table ${tableName}: FOREIGN KEY target column "${targets[n]}" is not declared in ${targetTable}`);
      }
    }
  }
}
if(sqliteFkLocalProblems.length) fail(
  "schema.sql declares structurally invalid foreign keys:\n  - "+
  sqliteFkLocalProblems.join("\n  - ")
);
console.log(`SQLite schema foreign-key structure: checked ${sqlTablesArr.length} table(s); local/target columns are declared`);

// SQLITE-DATA-ARCHITECTURE.md groups the tables by concern, and that grouping drifted the moment
// project_briefs and user_contributions were added: the tables existed in schema.sql and in DATA-MODEL.md
// while the architecture document still listed neither. Nothing compared the two, so an external audit found
// it. The grouping is prose, so it is checked by requiring every real table to appear somewhere in it.
const sqliteDoc=readText("docs/SQLITE-DATA-ARCHITECTURE.md");
const undocumented=[...sqlTables].filter(t=>!new RegExp(`\\b${t}\\b`).test(sqliteDoc));
if(undocumented.length) fail(`Table(s) in schema.sql are absent from docs/SQLITE-DATA-ARCHITECTURE.md, so the documented table grouping is stale:\n  - ${undocumented.join("\n  - ")}\nAdd each to its concern group in that document.`);

// --- Registry conformance against its own schema. payloads.json required `errors` on every operation while
// every operation carried an undeclared `owner`, so the file did not satisfy payloads.schema.json on any of
// its 58 operations and nothing detected it. The gate validated that the file existed, never its contents.
// This is a shape check rather than a full JSON Schema evaluation, which is enough for these two files and
// honest about being so: it enforces required keys, forbidden keys, and key types.
const payloadSchema=read("schemas/tauri-bridge-v1/payloads.schema.json");
const typeOk=(v,t)=>t==="string"?typeof v==="string":t==="boolean"?typeof v==="boolean":t==="array"?Array.isArray(v):t==="object"?(v!==null&&typeof v==="object"&&!Array.isArray(v)):true;
const opProblems=[];
const checkOperation=(section,def,name)=>{
  const where=`${section}.${name}`;
  for(const key of def.required ?? []) if(!(key in payloadRegistry[section][name])) opProblems.push(`${where} is missing required key "${key}"`);
  for(const key of Object.keys(payloadRegistry[section][name])) if(def.properties && !(key in def.properties)) opProblems.push(`${where} declares "${key}", which ${name ? "payloads.schema.json" : ""} does not permit`);
  for(const [key,spec] of Object.entries(def.properties ?? {})){
    if(!(key in payloadRegistry[section][name])) continue;
    const v=payloadRegistry[section][name][key];
    const t=spec.type;
    if(typeof t==="string" && !typeOk(v,t)) opProblems.push(`${where}.${key} must be ${t}, found ${Array.isArray(v)?"array":typeof v}`);
    if(t==="array" && Array.isArray(v) && spec.minItems!==undefined && v.length<spec.minItems) opProblems.push(`${where}.${key} needs at least ${spec.minItems} item(s)`);
  }
  if(def.properties?.request_fields && Array.isArray(payloadRegistry[section][name].request_fields)){
    for(const f of payloadRegistry[section][name].request_fields){
      for(const key of (def.properties.request_fields.items?.required ?? [])) if(!(key in f)) opProblems.push(`${where}.request_fields entry ${JSON.stringify(f.name ?? "?")} is missing "${key}"`);
    }
  }
};
for(const [section,defName] of [["commands","operation"],["queries","query_operation"]]){
  const def=payloadSchema.$defs[defName];
  for(const name of Object.keys(payloadRegistry[section] ?? {})) checkOperation(section,def,name);
}
if(opProblems.length) fail(opProblems.length+" payload registry conformance problem(s):\n  - "+opProblems.join("\n  - "));

// Request payload types must resolve. payloads.json named a type per operation - 116 request and response
// names - but none of them had a definition anywhere, so the entire input surface of the Control Room was
// structurally unvalidated and an implementation agent could satisfy create_project with
// {"initial_brief":" "}. Only types actually referenced by a declared request field are required to resolve:
// inventing payload types for all 116 names would be a declaration nothing consumes.
const payloadTypes=read("schemas/tauri-bridge-v1/payload-types.json").types ?? {};
const scalarTypes=new Set(["string","integer","number","boolean","object","array","null"]);
const baseType=(t)=>String(t).replace(/\[\]$/,"");
const unresolvedTypes=[];
const referencedTypes=new Set();
for(const section of ["commands","queries"]){
  for(const [name,op] of Object.entries(payloadRegistry[section] ?? {})){
    for(const f of op.request_fields ?? []){
      if(!f.type) continue;
      referencedTypes.add(baseType(f.type));
      if(scalarTypes.has(baseType(f.type))) continue;
      if(!(baseType(f.type) in payloadTypes)) unresolvedTypes.push(`${section}.${name} field "${f.name}" references type "${baseType(f.type)}", which payload-types.json does not define`);
    }
  }
}
if(unresolvedTypes.length) fail(`Tauri request field type(s) do not resolve:\n  - ${unresolvedTypes.join("\n  - ")}\nDefine the type in schemas/tauri-bridge-v1/payload-types.json. A type name that resolves to nothing leaves the operation's input unvalidated.`);

// The declared request field list must agree with the payload type it points at, or the two drift apart and an
// implementation agent cannot tell which is authoritative.
const typeDrift=[];
for(const section of ["commands","queries"]){
  for(const [name,op] of Object.entries(payloadRegistry[section] ?? {})){
    const schema=payloadTypes[op.request];
    if(!schema) continue;
    const required=new Set(schema.required ?? []);
    const declared=new Map((op.request_fields ?? []).map(f=>[f.name,f]));
    for(const [fieldName,field] of declared){
      if(!(fieldName in (schema.properties ?? {}))){
        typeDrift.push(`${section}.${name} declares field "${fieldName}" which ${op.request} does not define`);
        continue;
      }
      if(Boolean(field.required)!==required.has(fieldName)){
        typeDrift.push(`${section}.${name} marks "${fieldName}" required=${Boolean(field.required)} but ${op.request} treats it as ${required.has(fieldName)?"required":"optional"}`);
      }
    }
    for(const r of required) if(!declared.has(r)) typeDrift.push(`${section}.${name} does not declare required field "${r}" of ${op.request}`);
  }
}
if(typeDrift.length) fail(`Tauri request_fields disagree with the payload type they reference:\n  - ${typeDrift.join("\n  - ")}`);

// --- The declared request fields must be the arguments the handler actually accepts, under the names the wire
// actually uses (DEC-056).
//
// `payloads.json` declares each operation's request fields, and `payload-types.json` declares their types, but
// neither was ever compared with the Rust function that receives them. The casing divergence proved the cost:
// `create_project` accepted `local_path` and the wire sent `localPath`, and both declarations were satisfied
// because nothing looked at the function. The wire name of an argument is decided by the command's
// `rename_all`, whose Tauri default is camelCase, so the default is modelled here rather than assumed away.
const camelCase=(name)=>name.replace(/_([a-z0-9])/g,(_,c)=>c.toUpperCase());
const renameConventions={
  snake_case:(name)=>name,
  camelCase,
  lowercase:(name)=>name.toLowerCase(),
  UPPERCASE:(name)=>name.toUpperCase(),
  PascalCase:(name)=>name.charAt(0).toUpperCase()+camelCase(name).slice(1),
  "kebab-case":(name)=>name.replace(/_/g,"-"),
};
/** The wire name of a Rust argument, given the command's `rename_all`. */
const wireName=(convention,name)=>{
  if(convention===null) return camelCase(name); // Tauri's default for command arguments.
  const fn=renameConventions[convention];
  if(!fn) fail(
    `${BRIDGE_RUST}: a #[tauri::command] declares rename_all = "${convention}", which this gate cannot model.\n`+
    `The gate derives the wire name of each argument from it, so an unmodelled convention means the argument `+
    `names are unchecked. Add it to renameConventions rather than leaving the request surface unchecked.`
  );
  return fn(name);
};
/** Split a Rust parameter list on top-level commas, so a generic type's own commas do not split an argument. */
const splitParams=(params)=>{
  const out=[];let depth=0,current="";
  for(const ch of params){
    if(ch==="<"||ch==="("||ch==="[") depth++;
    else if(ch===">"||ch===")"||ch==="]") depth--;
    if(ch===","&&depth===0){ out.push(current); current=""; continue; }
    current+=ch;
  }
  if(current.trim()) out.push(current);
  return out.map(s=>s.trim()).filter(Boolean);
};
const requestProblems=[];
const optionalFieldsNotAccepted=[];
for(const command of commandFns){
  const declared=payloadRegistry.commands?.[command.name]?.request_fields;
  if(!declared){ continue; }
  const renameAll=/rename_all\s*=\s*"([^"]+)"/.exec(command.args)?.[1] ?? null;
  const accepted=[];
  for(const param of splitParams(command.params)){
    const colon=param.indexOf(":");
    if(colon<0) continue;
    const rustName=param.slice(0,colon).trim();
    const rustType=param.slice(colon+1).trim();
    // Tauri injects state and windows; they are not request fields and never appear on the wire.
    if(/^State\b|^Window\b|^AppHandle\b|^WebviewWindow\b/.test(rustType)) continue;
    accepted.push({wire:wireName(renameAll,rustName),rustName,optional:rustType.startsWith("Option<")});
  }
  const acceptedNames=accepted.map(a=>a.wire);
  const declaredByName=new Map(declared.map(f=>[f.name,f]));
  for(const arg of accepted){
    if(!declaredByName.has(arg.wire)) requestProblems.push(
      `commands.${command.name} accepts the argument "${arg.wire}", which its declared request_fields do not contain`+
      (arg.wire!==arg.rustName?` (the Rust argument is "${arg.rustName}"; rename_all decides the wire name)`:"")
    );
    else if(Boolean(declaredByName.get(arg.wire).required)===arg.optional) requestProblems.push(
      `commands.${command.name} accepts "${arg.wire}" as ${arg.optional?"Option<T> (optional)":"a required value"} but declares it required=${Boolean(declaredByName.get(arg.wire).required)}`
    );
  }
  for(const field of declared){
    if(acceptedNames.includes(field.name)) continue;
    // Required-but-unaccepted is a failure: the contract says a caller must send it and the handler cannot
    // receive it. Optional-but-unaccepted is reported instead, because it is the contract leading
    // implementation - the same asymmetry DEC-053 applies to whole operations.
    if(field.required) requestProblems.push(
      `commands.${command.name} declares the required request field "${field.name}", which the handler does not accept`
    );
    else optionalFieldsNotAccepted.push(`commands.${command.name}.${field.name}`);
  }
}
if(requestProblems.length) fail(
  `Tauri handler arguments disagree with the declared request fields (DEC-056):\n  - ${requestProblems.join("\n  - ")}\n`+
  `The contract is the authority (AGENTS.md section 5): declare the field, or stop accepting it.`
);

// --- Initial project creation must carry its intent anchor. DEC-030 makes the ProjectBrief the canonical
// representation of user intent; CONTROL-ROOM-DESIGN.md states the Initial Intake Composer persists the
// stated intent as the first brief version. If create_project stops declaring a required initial brief, a
// project can be created with no intent, which is the state the analysis-anchor rule exists to prevent.
const createProject=payloadRegistry.commands?.create_project;
if(!createProject) fail("payloads.json has no create_project command; project intake cannot be verified");
const briefFields=(createProject.request_fields ?? []).filter(f=>f.required && /brief/i.test(f.name ?? ""));
if(!briefFields.length) fail("create_project declares no required initial brief field. Project creation must persist the user's stated intent as ProjectBrief version 1 in the same transaction (DEC-030); a project must never exist without its intent anchor.");
if(!createProject.atomicity) fail("create_project declares no atomicity. Creating a project, its first brief, the PROJECT_CREATED event and the initial epoch is one domain transaction, not a sequence of calls.");

// --- Canonical artifact readability and registry/schema conformance.
// 22 of the 38 canonical artifacts declared in workspace.manifest.json:schema_sources were previously
// existence-checked only, including some of the highest-consequence registries in the repository. Two of
// them ship a sibling schema, so their contents can be checked for real rather than merely counted.
//
// This is a deliberately small JSON Schema subset - type, required, properties, additionalProperties, const,
// enum, minimum, items, minItems - because this repository has zero runtime dependencies and those are the
// keywords its own schemas actually use. It is not a general validator and does not pretend to be; it
// enforces what these files declare and reports honestly on the rest.
const jsonType=(v)=>Array.isArray(v)?"array":v===null?"null":typeof v==="number"&&Number.isInteger(v)?"integer":typeof v;
function validateSubset(value,schema,at,problems){
  if(schema.type){
    const want=Array.isArray(schema.type)?schema.type:[schema.type];
    const actual=jsonType(value);
    const ok=want.some(t=>t===actual||(t==="number"&&actual==="integer"));
    if(!ok){problems.push(`${at} must be ${want.join("|")}, found ${actual}`);return;}
  }
  if(schema.const!==undefined && value!==schema.const) problems.push(`${at} must equal ${JSON.stringify(schema.const)}`);
  if(schema.enum && !schema.enum.includes(value)) problems.push(`${at} must be one of ${JSON.stringify(schema.enum)}`);
  if(typeof value==="number" && schema.minimum!==undefined && value<schema.minimum) problems.push(`${at} must be >= ${schema.minimum}`);
  if(Array.isArray(value)){
    if(schema.minItems!==undefined && value.length<schema.minItems) problems.push(`${at} needs at least ${schema.minItems} item(s)`);
    if(schema.items) value.forEach((v,i)=>validateSubset(v,schema.items,`${at}[${i}]`,problems));
  }
  if(jsonType(value)==="object"){
    for(const key of schema.required ?? []) if(!(key in value)) problems.push(`${at} is missing required key "${key}"`);
    for(const [key,sub] of Object.entries(schema.properties ?? {})){
      if(key in value) validateSubset(value[key],sub,`${at}.${key}`,problems);
    }
    if(schema.additionalProperties===false && schema.properties){
      for(const key of Object.keys(value)) if(!(key in schema.properties)) problems.push(`${at} declares "${key}", which the schema does not permit`);
    }
  }
}
const conformanceProblems=[];
const validateAgainst=(file,schemaFile)=>{
  let instance,schema;
  try{ instance=read(file); }catch(e){ conformanceProblems.push(`${file} is not parseable JSON: ${e.message}`); return; }
  try{ schema=read(schemaFile); }catch(e){ conformanceProblems.push(`${schemaFile} is not parseable JSON: ${e.message}`); return; }
  validateSubset(instance,schema,file,conformanceProblems);
};
validateAgainst("schemas/error-v1/registry.json","schemas/error-v1/registry.schema.json");
validateAgainst("schemas/service-contracts-v1/registry.json","schemas/service-contracts-v1/registry.schema.json");
// configuration.schema.json describes the Configuration document (ui, agents, runtime, ...), not
// configuration-registry.json, which is an envelope of layers/precedence/groups. Validating the registry
// against it was a wrong pairing, not a finding.
validateAgainst("schemas/tauri-bridge-v1/payloads.json","schemas/tauri-bridge-v1/payloads.schema.json");

// --- The canonical error vocabulary must be complete, and it must agree with the protocol and with the code.
//
// `schemas/error-v1/registry.json` is where every explicit error code is declared, and
// `schemas/mcf-v2/error.schema.json` closes the MCF code, category, severity and retryability enums. Nothing
// compared the two, so the registry could name an MCF code the protocol does not define, and eleven codes the
// implementation actually produces were absent from the registry altogether (DEC-055). Both directions are
// closed below: what the registry claims must resolve in the protocol, and what the implementation produces
// must be registered.
const errorRegistry=read("schemas/error-v1/registry.json");
const errorCodes=errorRegistry.codes ?? {};
const mcfError=read("schemas/mcf-v2/error.schema.json");
const mcfEnum=(name)=>{
  const e=mcfError.properties?.[name]?.enum;
  if(!Array.isArray(e)) fail(
    `schemas/mcf-v2/error.schema.json declares no ${name} enum, so the error registry's ${name} values cannot be `+
    `resolved against the protocol. The registry draws its vocabulary from this schema, so a missing enum is a `+
    `failure rather than a skip.`
  );
  return new Set(e);
};
const mcfCodeEnum=mcfEnum("code");
const mcfCategoryEnum=mcfEnum("category");
const mcfSeverityEnum=mcfEnum("severity");
const mcfRetryEnum=mcfEnum("retryability");

const errorProblems=[];
const seenMcf=new Map();
const seenTauri=new Map();
for(const [key,entry] of Object.entries(errorCodes)){
  // A null mcf_code is the explicit statement that the code has no MCF counterpart - it is raised and consumed
  // inside the desktop boundary and never crosses the protocol. A non-null one must be a real protocol code,
  // and two registry codes may not share one: this registry maps one MCF code to one registry code, and a
  // many-to-one mapping is not modelled anywhere.
  if(entry.mcf_code!==null){
    if(!mcfCodeEnum.has(entry.mcf_code)) errorProblems.push(
      `${key}.mcf_code "${entry.mcf_code}" is not a member of the MCF error code enum in schemas/mcf-v2/error.schema.json`
    );
    else if(seenMcf.has(entry.mcf_code)) errorProblems.push(
      `${key}.mcf_code "${entry.mcf_code}" is already declared by ${seenMcf.get(entry.mcf_code)}; one MCF code maps to one registry code`
    );
    else seenMcf.set(entry.mcf_code,key);
  }
  if(seenTauri.has(entry.tauri_code)) errorProblems.push(
    `${key}.tauri_code "${entry.tauri_code}" is already declared by ${seenTauri.get(entry.tauri_code)}`
  );
  else seenTauri.set(entry.tauri_code,key);
  if(!mcfCategoryEnum.has(entry.category)) errorProblems.push(
    `${key}.category "${entry.category}" is not a member of the MCF error category enum`
  );
  if(!mcfSeverityEnum.has(entry.severity)) errorProblems.push(
    `${key}.severity "${entry.severity}" is not a member of the MCF error severity enum`
  );
  if(!mcfRetryEnum.has(entry.retryability)) errorProblems.push(
    `${key}.retryability "${entry.retryability}" is not a member of the MCF error retryability enum`
  );
  if(typeof entry.meaning!=="string"||entry.meaning.trim()==="") errorProblems.push(
    `${key} declares no meaning, so the code is a name nothing downstream can act on`
  );
}

// What the implementation can actually produce. A code literal is collected from two shapes: a `code: "..."`
// field, which is how both the Rust and the TypeScript sides build an error payload, and the `=> "..."` arms of
// the mappings that turn a domain rejection into a code. The arm scan is scoped to those two mappings rather
// than run over every `=>` arm in the repository, because enum-to-string mappings elsewhere (council roles,
// budget kinds, decision classes, claim grades) use the identical shape for values that are not error codes,
// and a scan that reported those as unregistered error codes would be wrong on every run.
// The Rust shell lives in `apps/desktop/src-tauri/src`, not under `apps/desktop/src`, so it has to be named
// separately. Getting that wrong is silent - the scan simply finds fewer codes - which is why the mapping-site
// lookup below fails closed when a file it must read was never scanned.
const scanFiles=walkFiles("crates").concat(walkFiles("apps/desktop/src"),walkFiles("apps/desktop/src-tauri/src"))
  .filter(f=>/\.(rs|ts|tsx)$/.test(f))
  .filter(f=>!/\/generated\//.test(f));
const scannedText=new Map(scanFiles.map(f=>[f,stripCommentsForScan(readText(f))]));
const emitted=new Map();
const recordEmitted=(code,file)=>{ if(!emitted.has(code)) emitted.set(code,file); };
for(const [file,src] of scannedText){
  for(const m of src.matchAll(/\bcode\s*:\s*"([A-Z][A-Z0-9_]*)"/g)) recordEmitted(m[1],file);
}
const mappingSites=[
  ["crates/workspace/src/validation.rs",/pub\s+fn\s+code\s*\([^)]*\)\s*->\s*&'static\s+str\s*\{/],
  ["crates/bus/src/error.rs",/pub\s+fn\s+code\s*\(\s*&self\s*\)\s*->\s*&'static\s+str\s*\{/],
  ["apps/desktop/src-tauri/src/main.rs",/impl\s+From<ProjectValidationError>\s+for\s+CommandError\s*\{/],
  ["apps/desktop/src-tauri/src/main.rs",/impl\s+From<(?:[A-Za-z_][A-Za-z0-9_]*::)*BusError>\s+for\s+CommandError\s*\{/],
];
// A delegating mapping produces the codes of the mapping it delegates to, so its body holds no literals for the
// scan above to read. That makes the presence check alone weak: replacing the call with a hardcoded string would
// keep the site locatable while producing a code the gate never read. Each entry here names the call its site
// must still make, keyed by the site's own regex source so the two cannot drift apart.
const delegatingSites=new Map([
  ["impl\\s+From<(?:[A-Za-z_][A-Za-z0-9_]*::)*BusError>\\s+for\\s+CommandError\\s*\\{","code()"],
]);
for(const [file,openRe] of mappingSites){
  const src=scannedText.get(file);
  if(src===undefined) fail(`${file} is one of the error-code mappings this check scans, but the file was not read.`);
  const open=openRe.exec(src);
  if(!open) fail(
    `${file}: could not find the error-code mapping this check scans (${openRe}).\n`+
    `A mapping the gate cannot locate is unchecked, so this is a failure rather than a skip.`
  );
  // Brace-matched body, so the scan covers exactly the mapping and nothing after it.
  let depth=0,end=-1;
  for(let i=open.index+open[0].length-1;i<src.length;i++){
    if(src[i]==="{") depth++;
    else if(src[i]==="}"&&--depth===0){ end=i; break; }
  }
  if(end<0) fail(`${file}: the error-code mapping's braces are unbalanced, so the codes it produces cannot be read.`);
  const body=src.slice(open.index,end);
  const required=delegatingSites.get(openRe.source);
  if(required!==undefined&&!body.includes(required)) fail(
    `${file}: the ${openRe} mapping no longer calls ${required}, so it must be producing codes this check never reads.\n`+
    `Either delegate to the mapping that owns those codes, or list them as literals so they are scanned. A mapping `+
    `whose output the gate cannot read is unchecked, which is why this fails rather than warns.`
  );
  for(const m of body.matchAll(/=>\s*"([A-Z][A-Z0-9_]*)"/g)) recordEmitted(m[1],file);
}
for(const [code,file] of emitted) if(!(code in errorCodes)) errorProblems.push(
  `${file} produces the error code "${code}", which schemas/error-v1/registry.json does not register`
);
if(errorProblems.length) fail(
  `The canonical error registry disagrees with the protocol or with the implementation (DEC-055):\n  - ${errorProblems.join("\n  - ")}\n`+
  `Register the code in schemas/error-v1/registry.json, or stop producing it. An error code that is not in the registry has no stated category, retryability, severity or meaning.`
);

// Reported, not failed. The wire carries the canonical registry key (EMPTY_FIELD); `tauri_code` is the
// namespaced spelling (MAYASABA_EMPTY_FIELD), and nothing in the repository emits or consumes one. Whether the
// wire should carry `tauri_code` instead is a second breaking wire change and a decision of its own, so this
// reports the gap rather than normalizing it (DEC-055).
const emittedTauriCodes=Object.values(errorCodes)
  .map(e=>e.tauri_code)
  .filter(name=>typeof name==="string"&&[...scannedText.values()].some(src=>src.includes(name)))
  .length;
const errorRegistryReport={registered:Object.keys(errorCodes).length,emitted:emitted.size,tauriCodesEmitted:emittedTauriCodes};

// Every canonical artifact must at least be readable in its declared form. This is weak but it is not
// nothing: an unreadable or malformed registry that nothing opens is exactly how the previous divergence
// survived, and parsing is the cheapest check that would have caught it.
const manifestForCoverage=read("workspace.manifest.json");
const canonicalAll=manifestForCoverage.schema_sources ?? [];
const unreadable=[];
for(const f of canonicalAll){
  if(f.endsWith(".sql")||f.endsWith(".md")){ if(!exists(f)) unreadable.push(f); continue; }
  try{ readUncounted(f); }catch(e){ unreadable.push(`${f} (${e.message})`); }
}
if(unreadable.length) fail(`Canonical artifact(s) declared in workspace.manifest.json:schema_sources are unreadable:\n  - ${unreadable.join("\n  - ")}`);

// The service registry and the workspace manifest both describe application services. If they disagree, an
// implementation agent cannot tell which crate owns a service, and Tauri command ownership resolves against
// the manifest.
const serviceRegistry=read("schemas/service-contracts-v1/registry.json");
const declaredServices=Object.keys(serviceRegistry.services ?? serviceRegistry.contracts ?? {});
const manifestServices=Object.keys(read("workspace.manifest.json").application_services ?? {});
const serviceDrift=declaredServices.filter(s=>!manifestServices.includes(s)).map(s=>`${s} is in service-contracts-v1/registry.json but not in workspace.manifest.json application_services`)
  .concat(manifestServices.filter(s=>!declaredServices.includes(s)).map(s=>`${s} is in workspace.manifest.json application_services but not in service-contracts-v1/registry.json`));
if(serviceDrift.length) fail(`Service registry and workspace manifest disagree on ${serviceDrift.length} service(s):\n  - ${serviceDrift.join("\n  - ")}`);

if(conformanceProblems.length) fail(conformanceProblems.length+" registry/schema conformance problem(s):\n  - "+conformanceProblems.join("\n  - "));

// Canonical schema sources are an allowlist, not a description. Without this, a future agent could create
// schemas/foo-v1/ and treat it as authoritative simply by not mentioning it anywhere the gate looks:
// DESIGN-GOVERNANCE forbids introducing a second authority, and an unregistered authority is exactly how one
// gets introduced quietly. Every schema directory under schemas/ must either appear in schema_sources or be
// declared here as deliberately non-canonical, with a reason.
const declaredNonCanonical=new Map([
  ["schemas/mcf-v2/conformance","conformance fixtures and expectations, not a contract surface"],
  ["schemas/mcf-v2/fixtures","test fixtures, not a contract surface"],
]);
const canonicalDirs=read("workspace.manifest.json").schema_sources ?? [];
const unregistered=[...new Set(fs.readdirSync(path.join(root,"schemas"),{withFileTypes:true}).filter(d=>d.isDirectory()).map(d=>`schemas/${d.name}`))]
  .filter(dir=>!canonicalDirs.some(f=>f.startsWith(`${dir}/`))&&!declaredNonCanonical.has(dir));
if(unregistered.length) fail(`Schema director(y|ies) exist that are absent from workspace.manifest.json:schema_sources:\n  - ${unregistered.join("\n  - ")}\nRegister every canonical file in schema_sources, or declare the directory non-canonical in verify.mjs with a reason. An unregistered schema directory is an authority nobody reviews.`);

// Per-file registration. The directory check above let 21 real MCF contract files sit outside the allowlist
// unnoticed, because a directory counts as registered if any one of its files is. Every file a package
// manifest declares canonical must itself appear in schema_sources, or it exists but nothing parses it,
// drift-checks it or reports on it.
//
// The test is "named in a package manifest", not "exists on disk". A file that exists but is declared nowhere
// and consumed by nothing is an orphan, which is a different problem from an unregistered canonical file, and
// conflating the two would force dead files into the canonical set. schemas/recovery-v1/recovery-events.json
// is exactly that case: it declares its own authority and is referenced by nothing.
const registeredSet=new Set(canonicalDirs);
const manifests=fs.readdirSync(path.join(root,"schemas"),{withFileTypes:true})
  .filter(d=>d.isDirectory() && fs.existsSync(path.join(root,"schemas",d.name,"manifest.json")))
  .map(d=>`schemas/${d.name}/manifest.json`);
const declaredCanonical=new Set();
for(const mf of manifests){
  let parsed;
  try{ parsed=JSON.parse(fs.readFileSync(path.join(root,mf),"utf8")); }catch{ continue; }
  const collect=(value)=>{
    if(Array.isArray(value)){ for(const v of value){ if(typeof v==="string") collectOne(v); } }
  };
  const collectOne=(name)=>{ if(typeof name==="string" && name.includes(".")) declaredCanonical.add(`schemas/${mf.split("/")[1]}/${name}`); };
  for(const value of Object.values(parsed)) collect(value);
}
const unregisteredFiles=[...declaredCanonical].filter(f=>!registeredSet.has(f));
if(unregisteredFiles.length) fail(`${unregisteredFiles.length} file(s) are declared canonical by a package manifest but absent from workspace.manifest.json:schema_sources:\n  - ${unregisteredFiles.sort().join("\n  - ")}\nRegister each one. A declared canonical file that is unregistered is parsed by nothing, checked by nothing, and can drift silently.`);

// --- Repeated entries in a canonical schema's own `required` or `enum` array.
// `envelope.schema.json` listed `authorization_context` twice in its conditional `required`: seven entries, six
// unique. JSON Schema requires the elements of `required` to be unique, so the file was not a conformant schema
// at all - and nothing read it. It was absent from schema_sources, so neither the consumer-coverage check nor
// this file's readability sweep opened it. No generated diff could reveal it either, because the generator
// dedupes that list before emitting a constant, so the staleness check stayed green over it.
//
// A duplicate is invisible to every set comparison: membership is identical whether a name appears once or
// twice. That is why the message and event lists needed an explicit repeat test, and why this one does too. It
// runs over every canonical JSON rather than only the envelope, because the real defect class is "a canonical
// artifact that nothing reads", and the envelope was merely the instance that happened to be found.
const repeatedArrayEntries=[];
const checkUniqueArrays=(node,at,file)=>{
  if(Array.isArray(node)){ node.forEach((v,i)=>checkUniqueArrays(v,at?`${at}[${i}]`:`[${i}]`,file)); return; }
  if(node&&typeof node==="object"){
    for(const [key,value] of Object.entries(node)){
      const where=at?`${at}.${key}`:key;
      if((key==="required"||key==="enum")&&Array.isArray(value)){
        const repeats=[...new Set(value.filter((v,i)=>value.indexOf(v)!==i))];
        if(repeats.length) repeatedArrayEntries.push(`${file}:${where} repeats ${repeats.map(r=>JSON.stringify(r)).join(", ")}`);
      }
      checkUniqueArrays(value,where,file);
    }
  }
};
for(const f of canonicalAll){
  if(!f.endsWith(".json")) continue;
  checkUniqueArrays(readUncounted(f),"",f);
}
if(repeatedArrayEntries.length) fail(`${repeatedArrayEntries.length} repeated entry/entries in a canonical schema's own required/enum array:\n  - ${repeatedArrayEntries.join("\n  - ")}\nJSON Schema requires the elements of "required" to be unique. A repeated entry is drift in a canonical artifact, and a set comparison cannot see it.`);

// --- The envelope's generated constants must agree with the schema they encode, checked here in-process.
// The generator's own --check mode is not sufficient on its own. That mode compares committed bytes against a
// re-derivation from the same schema, so it proves staleness, not correctness: a generator edited to read the
// wrong field, or to omit one, re-derives consistently and passes it. This compares the raw contract against the
// emitted constants, which is what catches a wrong or dropped generator input. It also makes this gate a genuine
// in-process consumer of envelope.schema.json and identity.schema.json rather than merely a lister of them.
//
// The repeated-entry check above is what catches a duplicate in the conditional `required`; this block compares
// against the deduped set deliberately, because that is what the generator emits. The two checks cover different
// halves of the same defect: a repeated entry in the contract, and a generator that misreads the contract.
const envelopeSchema=read("schemas/mcf-v2/envelope.schema.json");
const identitySchema=read("schemas/mcf-v2/identity.schema.json");
const envelopeRs=readText("crates/protocol/src/generated/envelope.rs");
const generatedList=(name)=>{
  const m=envelopeRs.match(new RegExp("pub const "+name+": &\\[&str\\] = &\\[([^\\]]*)\\]","s"));
  if(!m) return null;
  return m[1].trim()===""?[]:JSON.parse("["+m[1]+"]");
};
const constantProblems=[];
const sameList=(a,b)=>Array.isArray(a)&&Array.isArray(b)&&a.length===b.length&&a.every((v,i)=>v===b[i]);
const expectConstant=(name,expected)=>{
  const got=generatedList(name);
  if(got===null){ constantProblems.push(`generated/envelope.rs declares no ${name}`); return; }
  if(!sameList(got,expected)) constantProblems.push(`${name} is ${JSON.stringify(got)}, but the contract implies ${JSON.stringify(expected)}`);
};
const envelopeConditional=envelopeSchema.allOf?.[0]??{};
expectConstant("REQUIRED_FIELDS",envelopeSchema.required);
expectConstant("OPTIONAL_FIELDS",Object.keys(envelopeSchema.properties).filter(k=>!envelopeSchema.required.includes(k)));
expectConstant("MATERIAL_ACTION_MESSAGE_TYPES",envelopeConditional.if?.properties?.message_type?.enum??[]);
expectConstant("MATERIAL_REQUIRED_FIELDS",[...new Set(envelopeConditional.then?.required??[])]);
expectConstant("AUTHORIZATION_CONTEXT_REQUIRED_FIELDS",envelopeSchema.properties.authorization_context.required);
expectConstant("AUTHORIZATION_CONTEXT_FIELDS",Object.keys(envelopeSchema.properties.authorization_context.properties));
expectConstant("SECURITY_REQUIRED_FIELDS",envelopeSchema.properties.security.required);
expectConstant("SECURITY_FIELDS",Object.keys(envelopeSchema.properties.security.properties));
expectConstant("IDENTITY_REQUIRED_FIELDS",identitySchema.required);
expectConstant("IDENTITY_FIELDS",Object.keys(identitySchema.properties));
expectConstant("CHANNELS",envelopeSchema.properties.channel.enum);
expectConstant("PHASES",envelopeSchema.properties.phase.enum);
if(constantProblems.length) fail(constantProblems.length+" generated envelope constant(s) disagree with the contract:\n  - "+constantProblems.join("\n  - ")+"\nRun: npm run codegen:protocol, then check the generator with the same edit.");

// --- Recovery vocabulary ownership (DEC-017), and what is deliberately not enforced here.
// schemas/recovery-v1/recovery-events.json declared the recovery state vocabulary a second time: its 13
// `states` were identical, element for element, to recovery.schema.json:properties.state.enum. One concept
// therefore had two canonical declarations in the allowlist and nothing compared them, which is the
// shadow-source pattern DEC-017 prohibits. The duplicate is removed and this check keeps it removed - a
// re-added `states` list fails here rather than quietly becoming a second owner again.
//
// The `events` list is checked for a self-consistent `authority` and for repeated entries, and its members are
// resolved against the two vocabularies that could legitimately declare them: the canonical MCF event enum and
// the Tauri UI event enum. That resolution is REPORTED rather than enforced, and the reason is stated rather
// than implied. RECOVERY_STEP_CHANGED and RECOVERY_BLOCKED are declared in neither; registering them is a
// recovery-subsystem decision, because a UI event needs a bridge enum member, a payload entry, an owning
// service and a regenerated bridge. Reporting keeps the gap visible on every run without making the repository
// uncommittable, which a blocking version would do immediately. The limitation is stated so the green is not
// read as "every recovery event is registered".
const recoveryEvents=read("schemas/recovery-v1/recovery-events.json");
const recoverySchema=read("schemas/recovery-v1/recovery.schema.json");
const recoveryProblems=[];
if(recoveryEvents.states!==undefined) recoveryProblems.push("schemas/recovery-v1/recovery-events.json declares `states`; the recovery state vocabulary is owned by schemas/recovery-v1/recovery.schema.json:properties.state.enum, and a second declaration of one concept is the shadow-source pattern DEC-017 prohibits");
if(recoveryEvents.authority!=="schemas/recovery-v1/recovery-events.json") recoveryProblems.push(`schemas/recovery-v1/recovery-events.json declares authority ${JSON.stringify(recoveryEvents.authority)}, which is not its own path`);
const recoveryEventList=recoveryEvents.events;
if(!Array.isArray(recoveryEventList)||recoveryEventList.length===0) recoveryProblems.push("schemas/recovery-v1/recovery-events.json declares no events");
else {
  const repeats=[...new Set(recoveryEventList.filter((e,i)=>recoveryEventList.indexOf(e)!==i))];
  if(repeats.length) recoveryProblems.push(`schemas/recovery-v1/recovery-events.json repeats event(s): ${repeats.join(", ")}`);
}
const recoveryStates=recoverySchema.properties?.state?.enum;
if(!Array.isArray(recoveryStates)||recoveryStates.length===0) recoveryProblems.push("schemas/recovery-v1/recovery.schema.json declares no state enum, but it owns the recovery state vocabulary");
if(recoveryProblems.length) fail(recoveryProblems.length+" recovery vocabulary problem(s):\n  - "+recoveryProblems.join("\n  - "));
const undeclaredRecoveryEvents=(recoveryEventList??[]).filter(e=>!events.includes(e)&&!bridge.properties.event_type.enum.includes(e));
if(undeclaredRecoveryEvents.length) console.log(`Recovery events declared in neither the MCF event enum nor the Tauri UI event enum (reported, not blocking; registering them is a recovery-subsystem decision): ${undeclaredRecoveryEvents.join(", ")}`);

// --- Council decision-quality contracts (DEC-052) against each other and against the DDL.
// Three vocabularies describe the same closed sets after this decision: the JSON Schemas under
// schemas/council-v1/, the SQLite CHECK constraints on the six new tables, and the mode plan in
// council-policies.json. Before these checks they were three independent lists that happened to agree -
// precisely the arrangement that let other vocabularies in this repository drift apart unnoticed. Each is now
// compared to a single expected set, and the DDL's CHECK text is read back out of schema.sql rather than
// assumed, so a contract change that forgets the table fails here.
const modeSelectionSchema=read("schemas/council-v1/mode-selection.schema.json");
const decisionOutcomeSchema=read("schemas/council-v1/decision-outcome.schema.json");
const councilPolicies=read("schemas/council-v1/council-policies.json");
const COUNCIL_MODES=["SOLO","REVIEW","FULL"];
const COUNCIL_DECISION_CLASSES=["ARCHITECTURE","STACK_TECHNOLOGY","IRREVERSIBLE","SECURITY","DATA_LOSS","ROUTINE"];
const COUNCIL_OUTCOME_STATUSES=["HELD","AMENDED","REVERSED","UNRESOLVED"];
const COUNCIL_OUTCOME_SOURCES=["VALIDATION_RESULT","REOPEN_DECISION","USER_SUPERSESSION"];
const COUNCIL_ROLES=["PROPOSER","SKEPTIC","VERIFIER"];
const COUNCIL_GRADES=["ASSUMPTION","CITED","VERIFIED"];
const councilProblems=[];
// Named sameVocab, not sameSet: the agent-set check above already binds that name to a different predicate.
const sameVocab=(a,b)=>Array.isArray(a)&&Array.isArray(b)&&a.length===b.length&&b.every(x=>a.includes(x));
const expectEnum=(schema,at,expected,label)=>{
  const got=at(schema);
  if(!sameVocab(got,expected)) councilProblems.push(`${label} is ${JSON.stringify(got)}, expected ${JSON.stringify(expected)}`);
};
expectEnum(modeSelectionSchema,s=>s.properties.mode.enum,COUNCIL_MODES,"mode-selection.schema.json mode");
expectEnum(modeSelectionSchema,s=>s.properties.decision_class.enum,COUNCIL_DECISION_CLASSES,"mode-selection.schema.json decision_class");
expectEnum(modeSelectionSchema,s=>s.properties.override_source.enum,["NONE","USER"],"mode-selection.schema.json override_source");
expectEnum(decisionOutcomeSchema,s=>s.properties.mode.enum,COUNCIL_MODES,"decision-outcome.schema.json mode");
expectEnum(decisionOutcomeSchema,s=>s.properties.decision_class.enum,COUNCIL_DECISION_CLASSES,"decision-outcome.schema.json decision_class");
expectEnum(decisionOutcomeSchema,s=>s.properties.status.enum,COUNCIL_OUTCOME_STATUSES,"decision-outcome.schema.json status");
expectEnum(decisionOutcomeSchema,s=>s.properties.source.enum,COUNCIL_OUTCOME_SOURCES,"decision-outcome.schema.json source");
if(decisionOutcomeSchema.properties?.informational_only?.const!==true) councilProblems.push("decision-outcome.schema.json must pin informational_only to const true; outcome data must never become an authority");
// The DDL's own vocabulary, parsed out of its CHECK clause. A missing clause is a failure, not a skip: free
// text is what these columns were before, and the CHECK is the only thing that closes them at rest.
const ddlVocab=(table,column)=>{
  const block=sqlText.match(new RegExp("CREATE TABLE IF NOT EXISTS "+table+" \\(([\\s\\S]*?)\\n\\);","m"));
  if(!block) return null;
  const clause=block[1].match(new RegExp(column+" TEXT NOT NULL CHECK\\("+column+" IN \\(([^)]*)\\)\\)"));
  return clause?[...clause[1].matchAll(/'([^']+)'/g)].map(m=>m[1]):null;
};
for(const [table,column,expected] of [
  ["council_mode_selections","mode",COUNCIL_MODES],
  ["council_mode_selections","decision_class",COUNCIL_DECISION_CLASSES],
  ["council_mode_selections","override_source",["NONE","USER"]],
  ["council_round_roles","role",COUNCIL_ROLES],
  ["council_claim_grades","grade",COUNCIL_GRADES],
  ["council_decision_outcomes","mode",COUNCIL_MODES],
  ["council_decision_outcomes","decision_class",COUNCIL_DECISION_CLASSES],
  ["council_decision_outcomes","status",COUNCIL_OUTCOME_STATUSES],
  ["council_decision_outcomes","source",COUNCIL_OUTCOME_SOURCES],
]){
  const got=ddlVocab(table,column);
  if(got===null){ councilProblems.push(`schema.sql ${table}.${column} has no CHECK(${column} IN (...)) constraint; an unconstrained column is a vocabulary nobody enforces`); continue; }
  if(!sameVocab(got,expected)) councilProblems.push(`schema.sql ${table}.${column} permits ${JSON.stringify(got)}, but the contract says ${JSON.stringify(expected)}`);
}
if(!/CHECK\(status <> 'HELD' OR validation_evidence_id IS NOT NULL\)/.test(sqlText)) councilProblems.push("schema.sql council_decision_outcomes must require validation_evidence_id when status is HELD");
if(!/FOREIGN KEY\(decision_id\) REFERENCES decisions\(decision_id\)/.test(sqlText)) councilProblems.push("schema.sql council_decision_outcomes must reference decisions(decision_id); no council record was linked to a decision before this one");
if(!/FOREIGN KEY\(validation_evidence_id\) REFERENCES evidence\(evidence_id\)/.test(sqlText)) councilProblems.push("schema.sql council_decision_outcomes must reference evidence(evidence_id) directly, because evidence_links links evidence to artifacts only");
// The policy artifact must agree with the schemas it governs.
const planModes=Object.keys(councilPolicies.mode_plan??{}).filter(k=>k!=="rule");
if(!sameVocab(planModes,COUNCIL_MODES)) councilProblems.push(`council-policies.json mode_plan names ${JSON.stringify(planModes)}, expected ${JSON.stringify(COUNCIL_MODES)}`);
if(councilPolicies.mode_plan?.SOLO?.requires_round!==false) councilProblems.push("council-policies.json must declare that SOLO opens no round");
if(typeof councilPolicies.mode_plan?.rule!=="string"||!/spine/.test(councilPolicies.mode_plan.rule)) councilProblems.push("council-policies.json must state that modes do not change the round's state path, because a phase-skipping mode would need a new off-spine edge");
const minLineages=councilPolicies.evidence?.minimum_lineage_groups_for_corroboration;
if(!(typeof minLineages==="number"&&minLineages>=2)) councilProblems.push(`council-policies.json minimum_lineage_groups_for_corroboration is ${JSON.stringify(minLineages)}; fewer than 2 lets one lineage corroborate itself`);
if(councilPolicies.evidence?.block_convergence_on_load_bearing_assumption!==true) councilProblems.push("council-policies.json must block convergence on a load-bearing assumption");
if(typeof councilPolicies.escalation!=="string"||councilPolicies.escalation.length===0) councilProblems.push("council-policies.json declares no escalation rule; exhaustion must produce an outcome rather than silent acceptance");
if(typeof councilPolicies.reporting?.minimum_sample_for_percentage!=="number") councilProblems.push("council-policies.json declares no minimum sample for percentage reporting");
if(councilProblems.length) fail(councilProblems.length+" council decision-quality contract problem(s):\n  - "+councilProblems.join("\n  - "));

// --- Bus retry/dispatch policy (DEC-058) against the machine it terminates through and against the crate.
// `bus-policies.json` is configuration, so nothing here locks its numbers: what is checked is that the numbers
// are usable, that the crate's shipped defaults are the same numbers, and - the check that matters - that the
// termination path the policy declares is an edge the `message_delivery` machine actually has. A policy that
// named a path the machine does not declare would be a contract describing behaviour the contract forbids, and
// it would be found only when a message exhausted its budget in production.
const busPolicies=read("schemas/mcf-v2/bus-policies.json");
const busPolicyProblems=[];
const positiveInt=(value,label)=>{
  if(!Number.isInteger(value)||value<1) busPolicyProblems.push(`${label} is ${JSON.stringify(value)}; it must be a positive integer`);
  return value;
};
const busMaxAttempts=positiveInt(busPolicies.dispatch?.max_attempts,"bus-policies.json dispatch.max_attempts");
positiveInt(busPolicies.dispatch?.batch_size,"bus-policies.json dispatch.batch_size");
const busBase=positiveInt(busPolicies.backoff?.base_seconds,"bus-policies.json backoff.base_seconds");
const busMultiplier=positiveInt(busPolicies.backoff?.multiplier,"bus-policies.json backoff.multiplier");
const busCap=positiveInt(busPolicies.backoff?.cap_seconds,"bus-policies.json backoff.cap_seconds");
if(busBase>busCap) busPolicyProblems.push(`bus-policies.json backoff.base_seconds (${busBase}) exceeds cap_seconds (${busCap}), so the first delay would already be capped and the multiplier would do nothing`);
for(const group of ["dispatch","backoff","terminal"]){
  if(typeof busPolicies[group]?.rule!=="string"||busPolicies[group].rule.length===0) busPolicyProblems.push(`bus-policies.json:${group} declares no rule; a policy number with no stated semantics is a number a reader has to guess at`);
}
if(typeof busPolicies.rule!=="string"||busPolicies.rule.length===0) busPolicyProblems.push("bus-policies.json declares no top-level rule");
// The backoff must be stated to be deterministic, because the design requires it and a jittered backoff would
// need a random source this crate deliberately does not have.
if(!/deterministic/.test(busPolicies.backoff?.rule??"")) busPolicyProblems.push("bus-policies.json backoff.rule must state that the delay is deterministic from stored state, which is what docs/MCF-V2-IMPLEMENTATION-DESIGN.md requires and what a jittered backoff would break");
// The termination path, checked against the machine rather than taken on trust.
// `state_mutation` is a sentence (`message_delivery.state = EXPIRED`), so the target state is read out of it
// rather than assumed to be the field's whole value.
const mutatedState=(mutation)=>/=\s*([A-Z_]+)\s*$/.exec(mutation??"")?.[1]??null;
const deliveryEdges=new Set((transition.transitions?.message_delivery??[])
  .map(t=>`${t.source_state}->${mutatedState(t.state_mutation)}`));
const declaredPath=busPolicies.terminal?.on_attempt_budget_exhausted;
if(declaredPath!=="QUEUED_TO_EXPIRED") busPolicyProblems.push(`bus-policies.json terminal.on_attempt_budget_exhausted is ${JSON.stringify(declaredPath)}; the accepted reading of the machine is that an exhausted sender terminates through QUEUED -> EXPIRED, so any other value needs a decision record rather than a policy edit`);
if(!deliveryEdges.has("QUEUED->EXPIRED")) busPolicyProblems.push("bus-policies.json terminates through QUEUED -> EXPIRED, which the message_delivery machine does not declare");
if(busPolicies.terminal?.dead_letter!==true) busPolicyProblems.push("bus-policies.json must require a dead letter when the attempt budget is exhausted, because the alternative loses the reason the message died");

// Every delivery edge the code advances through must be an edge the machine declares.
//
// The check reads the Rust rather than a list kept beside it. A list beside the code is a second source of truth
// for the same fact, free to disagree with the code it describes - which is the failure mode this whole gate
// exists to prevent, so a check built that way would be the disease dressed as the cure. Reading `advance_in` call
// sites means a state name the code invents cannot pass by being absent from a hand-maintained list: it is absent
// from the machine, which is what is asked.
//
// The four state columns carry no `CHECK` constraint at the SQL level, so nothing below the gate stops an
// undeclared state being written. This is that stop.
const advanceCall = /advance_in\(\s*tx,\s*[^,]+,\s*"([A-Z_]+)",\s*"([A-Z_]+)"/g;
let advanceCalls = 0;
for (const file of ["crates/storage/src/lib.rs", "crates/bus/src/lib.rs"]) {
  // Read without recording coverage: `contentRead` is the evidence that a canonical JSON contract was checked,
  // and a `.rs` file is not one. Claiming coverage of a source file would inflate the coverage figure with reads
  // that enforce nothing about the contracts it counts.
  for (const match of fs.readFileSync(path.join(root, file), "utf8").matchAll(advanceCall)) {
    advanceCalls++;
    const edge = `${match[1]}->${match[2]}`;
    if (!deliveryEdges.has(edge)) busPolicyProblems.push(`${file} advances ${edge}, which the message_delivery machine does not declare`);
  }
}
// A guard against the check quietly passing: if the call shape changes, the regex stops matching and every edge
// would be "declared" because none was found. A check that cannot fail is not a check.
if (advanceCalls === 0) busPolicyProblems.push("no advance_in call was found in crates/storage or crates/bus, so the delivery edges the code advances through were not checked at all");
// The crate's shipped defaults, read back out of the Rust rather than assumed to match.
// Read as text, not through `read`: that helper parses JSON, and this is Rust source.
const busPolicySource=fs.readFileSync(path.join(root,"crates/bus/src/policy.rs"),"utf8");
const rustConst=(name)=>{
  const m=new RegExp(`pub const ${name}: i64 = (\\d+);`).exec(busPolicySource);
  return m?Number(m[1]):null;
};
const busDefaults=[
  ["MAX_ATTEMPTS",busMaxAttempts,"dispatch.max_attempts"],
  ["BATCH_SIZE",busPolicies.dispatch?.batch_size,"dispatch.batch_size"],
  ["BASE_SECONDS",busBase,"backoff.base_seconds"],
  ["MULTIPLIER",busMultiplier,"backoff.multiplier"],
  ["CAP_SECONDS",busCap,"backoff.cap_seconds"],
  ["MAX_PENDING",busPolicies.dispatch?.max_pending,"dispatch.max_pending"],
];
for(const [constant,expected,where] of busDefaults){
  const got=rustConst(constant);
  if(got===null) busPolicyProblems.push(`crates/bus/src/policy.rs declares no \`pub const ${constant}: i64 = <n>;\` for the gate to read; the shipped default and ${where} must be comparable rather than merely similar`);
  else if(got!==expected) busPolicyProblems.push(`crates/bus/src/policy.rs ${constant} is ${got} but bus-policies.json ${where} is ${expected}; the shipped default and the policy file must agree`);
}
if(busPolicyProblems.length) fail(busPolicyProblems.length+" bus policy problem(s):\n  - "+busPolicyProblems.join("\n  - "));

// --- The dispatcher's lane order against the contract's declared lane order (DEC-060).
// The due query ranks lanes with a `CASE`, so the order lives in SQL where no type system can see it. If the
// contract's lane order changes and the SQL does not, the bus silently serves the wrong traffic first - and the
// requirement it would break is the one that says emergency control and recovery traffic must not be blocked by
// bulk output. The lane names are read out of the SQL in order and compared with `registry.priority_lanes`.
const declaredLanes=read("schemas/mcf-v2/registry.json").priority_lanes;
const laneProblems=[];
const laneSource=fs.readFileSync(path.join(root,"crates/storage/src/lib.rs"),"utf8");
const laneBlock=/ORDER BY CASE json_extract\(m\.envelope_json, '\$\.priority'\)([\s\S]*?)ELSE \d+\s*\n\s*END/.exec(laneSource);
if(!laneBlock){
  laneProblems.push("crates/storage/src/lib.rs declares no `ORDER BY CASE json_extract(m.envelope_json, '$.priority')` lane ranking for the due query, so the dispatcher has no lane order to check");
} else {
  const sqlLanes=[...laneBlock[1].matchAll(/WHEN '([A-Z_]+)' THEN \d+/g)].map(m=>m[1]);
  if(JSON.stringify(sqlLanes)!==JSON.stringify(declaredLanes)){
    laneProblems.push(`the due query ranks lanes ${JSON.stringify(sqlLanes)} but schemas/mcf-v2/registry.json declares ${JSON.stringify(declaredLanes)}; a dispatcher that serves them in a different order serves the wrong traffic first`);
  }
}
if(laneProblems.length) fail(laneProblems.length+" dispatch lane-order problem(s):\n  - "+laneProblems.join("\n  - "));

// --- Generated Rust must match the contract it claims to encode.
// crates/protocol/src/generated/machines.rs is the typed surface of MCF-v2. If the contract changes and the
// crate is not regenerated, the crate silently encodes a different protocol from the one the gate validates -
// which would be the first place in this repository where two artifacts both claim authority and disagree.
// The generator's --check mode re-derives the file from machines[] and registry.json and compares bytes.
//
// Compilation is not what this check proves, and the two are complementary rather than substitutes: a compiler
// cannot detect that the contract has moved on. Compilation is covered separately - `crates/protocol/src/lib.rs`
// declares `pub mod generated`, so `cargo test --workspace` and `cargo clippy --workspace --all-targets` do
// compile machines.rs and envelope.rs. What this check adds is that the committed bytes are what the CURRENT
// contract implies. (An earlier version of this comment claimed no Rust toolchain was configured and that
// `cargo build` could not run; both were false by the time rustfmt and clippy were installed, and a stale
// statement about what is verified is itself a verification defect.)
const genCheck=(()=>{ try{ return execFileSync("node",["tools/codegen/generate-protocol.mjs","--check"],{cwd:root,encoding:"utf8",stdio:["ignore","pipe","pipe"]}); }catch(e){ return "FAILED: "+(e.stderr||e.message).toString().trim(); } })();
if(genCheck.startsWith("FAILED")) fail("crates/protocol generated code is stale or missing.\n"+genCheck+"\nRun: npm run codegen:protocol");

// The Tauri bridge surface gets the same treatment. The Control Room calls these commands, queries and events,
// and the TypeScript surface previously had no byte-level drift protection at all: verify.mjs checked that
// every bridge name had payload metadata, never that the generated file was current. Protocol and bridge
// generation are now at parity - and the Rust half is now generated too. `bridge.schema.json` had declared
// `x-codegen.rust_output` all along, while no generator emitted that file and no check read it: the contract
// named an output the tooling did not produce, so "generated and checked in" was an assertion with no
// mechanism behind it. One generator now owns both files and --check compares both.
const bridgeGenCheck=(()=>{ try{ return execFileSync("node",["tools/codegen/generate-bridge.mjs","--check"],{cwd:root,encoding:"utf8",stdio:["ignore","pipe","pipe"]}); }catch(e){ return "FAILED: "+(e.stderr||e.message).toString().trim(); } })();
if(bridgeGenCheck.startsWith("FAILED")) fail("the generated bridge surface is stale or missing.\n"+bridgeGenCheck+"\nRun: npm run codegen:bridge");

// The generated Rust surface is checked for staleness but is not compiled: no `mod` declaration in the shell
// references apps/desktop/src-tauri/src/generated/bridge.rs, so the build never reads it. That is reported
// rather than repaired here - wiring it into the crate is a shell change, not a gate change - and it is
// reported because a generated file that is current but never compiled reads as enforcement it is not.
console.log(/(^|\n)\s*(?:pub\s+)?mod\s+generated\b/.test(rustCode)
  ? "Generated bridge.rs: referenced by a `mod generated` declaration, so the compiler reads it"
  : "Generated bridge.rs: current, but referenced by no `mod` declaration in the shell, so it is not compiled (reported, not blocking)");

// --- Tracked-but-ignored files. .gitignore governs only UNTRACKED paths, so a rule added after files were
// already committed has no effect on them. That happened twice here: 3,057 files under target/ were committed
// by `git add -A` in 8c19151 before /target/ was ignored, and 12,459 files under .clj-kondo/.cache were
// committed before .clj-kondo/ was ignored in 5613e81. In both cases the ignore rule was present, the gate was
// green, and the repository still carried the files - so the rule read as protection it was not providing.
// `git ls-files -ci --exclude-standard` is exactly the question "is anything tracked while ignored", which no
// other check in this file asks.
//
// Reported as SKIPPED rather than passed when git is unavailable or this is not a work tree: a check that
// cannot run must not report success, which is the same reason the consumer-coverage check says which mode it
// used instead of silently passing. Confined to a single git call so it stays cheap on every commit.
const ignoredButTracked=(()=>{
  try {
    return execFileSync("git",["ls-files","-ci","--exclude-standard"],{cwd:root,encoding:"utf8",stdio:["ignore","pipe","ignore"]})
      .trim().split(/\r?\n/).filter(Boolean);
  } catch { return null; }
})();
if(ignoredButTracked===null){
  console.log("Tracked-but-ignored check: skipped (git unavailable, or this is not a git work tree)");
} else if(ignoredButTracked.length){
  const shown=ignoredButTracked.slice(0,20);
  // The fix is per top-level directory, so name the distinct ones rather than 12,459 individual commands.
  const dirs=[...new Set(ignoredButTracked.map(f=>f.split("/").slice(0,2).join("/")))];
  fail(
    `${ignoredButTracked.length} tracked file(s) are also matched by .gitignore, so the ignore rule has no effect on them.\n`+
    `  - ${shown.join("\n  - ")}`+
    (ignoredButTracked.length>shown.length?`\n  ... and ${ignoredButTracked.length-shown.length} more`:"")+
    `\nFix: git rm -r --cached <dir> for each of: ${dirs.join(", ")}`+
    `\n(--cached removes them from the index only, so the files stay on disk for the tools that own them.)`
  );
}

// --- Gate coverage self-report.
// This gate was strengthened across DEC-036..DEC-043 and each fix found real defects, which is also how it
// stayed silent about whole layers. It had never read schema.sql, so the durable layer was unverified while
// the gate reported itself fully green - and an external audit, not the gate, found the divergence. The
// strongest enforcement in the repository was blind to exactly the layer an implementation agent opens first.
//
// So coverage is declared and self-checked rather than assumed. An artifact listed as verified must actually
// have its contents read during this run; if the check that reads it is deleted or renamed, this fails rather
// than quietly downgrading the claim. That is what keeps this map from becoming the same stale assertion it
// exists to prevent - the recurring defect in this repository, where a documented thing and its enforcement
// drift apart unnoticed.
const coverageVerified=new Set([
  "schemas/mcf-v2/manifest.json",
  "schemas/mcf-v2/registry.json",
  "schemas/mcf-v2/transition-types.json",
  "schemas/mcf-v2/event-types.schema.json",
  "schemas/mcf-v2/message-types.schema.json",
  "schemas/mcf-v2/message-payloads.registry.json",
  "schemas/mcf-v2/event-payloads.registry.json",
  "schemas/mcf-v2/event-to-ui.registry.json",
  "schemas/mcf-v2/envelope.schema.json",
  "schemas/mcf-v2/identity.schema.json",
  "schemas/recovery-v1/recovery-events.json",
  "schemas/recovery-v1/recovery.schema.json",
  "schemas/council-v1/mode-selection.schema.json",
  "schemas/council-v1/council-policies.json",
  "schemas/council-v1/decision-outcome.schema.json",
  "schemas/agent-adapter-v1/native-event.schema.json",
  "schemas/agent-adapter-v1/native-to-mcf.registry.json",
  "schemas/agent-adapter-v1/native-transport-contract.json",
  "schemas/agent-adapter-v1/probe-result.schema.json",
  "schemas/agent-adapter-v1/adapter-types.schema.json",
  "schemas/doctor-v1/doctor-report.schema.json",
  "schemas/tauri-bridge-v1/bridge.schema.json",
  "schemas/tauri-bridge-v1/payloads.json",
  "schemas/tauri-bridge-v1/payloads.schema.json",
  "schemas/error-v1/registry.json",
  "schemas/mcf-v2/error.schema.json",
  "schemas/service-contracts-v1/registry.json",
  "schemas/sqlite-v1/schema.sql",
  "workspace.manifest.json",
  "docs/DATA-MODEL.md",
]);
const drift=[...coverageVerified].filter(f=>!contentRead.has(f));
if(drift.length) fail(`Gate coverage map claims these artifacts are verified, but this run did not read their contents:\n  - ${drift.join("\n  - ")}\nEither restore the check that reads them or remove them from the coverage map. A coverage claim that nothing enforces is the defect this block exists to prevent.`);
// Report verified and unverified against the same denominator. An earlier version counted every entry in the
// coverage map, including artifacts outside schema_sources, so the reported figures did not sum to the
// canonical total - a reporting bug in the mechanism meant to fix reporting blindness.
const canonicalList=read("workspace.manifest.json").schema_sources ?? [];
const verifiedInCanonical=canonicalList.filter(f=>coverageVerified.has(f));
const unverified=canonicalList.filter(f=>!coverageVerified.has(f));
gateCoverage={verified:verifiedInCanonical.length,canonical:canonicalList.length,unverified:unverified.length,parsed:canonicalList.length};

console.log("Mayasaba contract verification passed.");
console.log(`MCF messages: ${messages.length}; events: ${events.length}; transition machines: ${Object.keys(transition.machines).length}`);
console.log(`Tauri commands: ${bridge.properties.command.enum.length}; queries: ${bridge.properties.query.enum.length}; UI events: ${bridge.properties.event_type.enum.length}`);
// The error vocabulary's own figures, stated every run for the same reason as the bridge figures: a count that
// lives only in a document drifts away from the thing it counts.
console.log(`Error registry: ${errorRegistryReport.registered} codes registered; ${errorRegistryReport.emitted} produced by the implementation; ${errorRegistryReport.tauriCodesEmitted} of ${errorRegistryReport.registered} tauri_code values emitted anywhere (the wire carries the canonical registry key; reported, not blocking)`);
if(optionalFieldsNotAccepted.length) console.log(
  `Declared optional request fields the handler does not accept (reported, not blocking; the contract leads implementation): ${optionalFieldsNotAccepted.join(", ")}`
);
// The reported half of the two-way bridge gate. Stated every run, including when it is large, because a gap
// that is only visible in a document is a gap that drifts; the README figure and this line are the same fact.
console.log(`Bridge: ${bridgeImplemented.length} of ${declaredOps.size} declared operations implemented; ${bridgeUnimplemented.length} declared with no handler (reported, not blocking)`);
if(bridgeUnimplemented.length){
  const lines=[];let current="";
  for(const n of bridgeUnimplemented){
    if(current&&(current+", "+n).length>96){lines.push(current+",");current=n;}
    else current=current?current+", "+n:n;
  }
  if(current)lines.push(current);
  console.log(`Declared bridge operations with no handler:\n${lines.map(l=>"  "+l).join("\n")}`);
}
console.log(`Gate coverage: ${gateCoverage.verified} of ${gateCoverage.canonical} canonical artifacts invariant-checked; all ${gateCoverage.parsed} parsed for readability; ${gateCoverage.unverified} parse-only, no invariant enforced (reported, not blocking)`);
// Hook status is reported, never enforced. A pre-commit gate that checks whether the pre-commit gate
// is installed would need its own hook, so the recursion is stopped one level down: the mechanism
// cannot verify itself, but its absence is a reported fact in the gate's own output rather than a
// silent unknown. Informational on purpose - a contributor who deliberately opts out is not blocked.
let hooksPath=null;
try { hooksPath=execFileSync("git",["config","core.hooksPath"],{cwd:root,encoding:"utf8",stdio:["ignore","pipe","ignore"]}).trim(); } catch { /* unset, or not a git checkout */ }
console.log(hooksPath===".githooks"
  ? "Pre-commit gate: enabled (core.hooksPath=.githooks)"
  : "Pre-commit gate: NOT enabled"+(hooksPath?" (core.hooksPath="+hooksPath+", expected .githooks)":" (core.hooksPath unset)")+" - run: git config core.hooksPath .githooks");

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
// Four crate manifests - agents, bus, core and protocol - contained a literal backslash-n instead of real
// newlines, so TOML parsed them as one garbage line and cargo could not build them. The check above is a
// substring test, so it passed: the dependency names were present as text inside an unparseable file. A
// substring test cannot tell a working manifest from a broken one, so the structural requirements are
// checked directly. This is the same shape as the registry conformance gaps - a check that reads a file
// without validating that the file means anything.
const manifestProblems=[];
for(const [name,def] of Object.entries(wm.crates)){
  const cargo=readText(def.path+"/Cargo.toml");
  if(!cargo.includes("\n")) manifestProblems.push(`${name}: Cargo.toml has no real newline; it holds a literal \\n and is not valid TOML`);
  if(cargo.includes("\\n")) manifestProblems.push(`${name}: Cargo.toml contains a literal \\n sequence`);
  for(const key of ["[package]","name =","version.workspace = true","edition.workspace = true","[dependencies]"]){
    if(!cargo.includes(key)) manifestProblems.push(`${name}: Cargo.toml is missing required entry "${key}"`);
  }
  const declaredName=(cargo.match(/^name\s*=\s*"([^"]+)"/m)??[])[1];
  if(declaredName!==`mayasaba-${name}`) manifestProblems.push(`${name}: Cargo.toml declares package name "${declaredName}", expected "mayasaba-${name}"`);
  for(const dep of (def.depends||[])){
    if(!new RegExp(`^mayasaba-${dep}\\s*=`, "m").test(cargo)) manifestProblems.push(`${name}: dependency mayasaba-${dep} is not declared as a Cargo dependency entry`);
    if(!cargo.includes(`path = "../${dep}"`)) manifestProblems.push(`${name}: dependency mayasaba-${dep} does not declare the workspace-local path ../${dep}`);
  }
}
if(manifestProblems.length) fail(manifestProblems.length+" Rust crate manifest problem(s):\n  - "+manifestProblems.join("\n  - "));
