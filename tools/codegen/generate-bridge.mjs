import fs from "node:fs";
import path from "node:path";
const root=process.cwd();
const bridge=JSON.parse(fs.readFileSync(path.join(root,"schemas/tauri-bridge-v1/bridge.schema.json"),"utf8"));
const commands=bridge.properties.command.enum, queries=bridge.properties.query.enum, events=bridge.properties.event_type.enum;
const ts=`// GENERATED FILE — DO NOT EDIT.\n// Source: schemas/tauri-bridge-v1/bridge.schema.json\nexport const COMMANDS = ${JSON.stringify(commands,null,2)} as const;\nexport const QUERIES = ${JSON.stringify(queries,null,2)} as const;\nexport const EVENTS = ${JSON.stringify(events,null,2)} as const;\nexport type CommandName = typeof COMMANDS[number];\nexport type QueryName = typeof QUERIES[number];\nexport type EventName = typeof EVENTS[number];\n`;
fs.mkdirSync(path.join(root,"apps/desktop/src/generated"),{recursive:true}); fs.writeFileSync(path.join(root,"apps/desktop/src/generated/bridge.ts"),ts);
console.log("Generated bridge.ts");
