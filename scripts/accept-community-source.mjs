// Explicit local preference update, run only after the user accepts this source.
import { readFile,writeFile,rename,copyFile,mkdir } from 'node:fs/promises';
import { resolve,join } from 'node:path';
import { createRequire } from 'node:module';
import { runInNewContext } from 'node:vm';
const root=resolve(import.meta.dirname,'..');
const require=createRequire(join(root,'apps/desktop/package.json'));
const ts=require('typescript');
const compile=async(path,dependencies={})=>{
  const source=await readFile(path,'utf8');
  const output=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  const module={exports:{}};
  runInNewContext(output,{module,exports:module.exports,TextEncoder,require:name=>{if(!(name in dependencies))throw new Error(`Unexpected dependency ${name}`);return dependencies[name];}});
  return module.exports;
};
if(!process.argv.includes('--accepted-by-user'))throw new Error('Run only after explicit acceptance: --accepted-by-user');
if(!process.env.APPDATA)throw new Error('Windows APPDATA directory unavailable');
const types=await compile(join(root,'apps/desktop/src/types.ts'));
const dynamics=await compile(join(root,'apps/desktop/src/dynamics-validation.ts'));
const {validateSettings}=await compile(join(root,'apps/desktop/src/validation.ts'),{'./types':types,'./dynamics-validation':dynamics});
const explicitIndex=process.argv.indexOf('--settings-dir');
const directory=explicitIndex<0?join(process.env.APPDATA,'pl.mtc.tacticalassistant'):resolve(process.argv[explicitIndex+1]??'');
await mkdir(directory,{recursive:true});
let saved=null;
let existingPath;
for(const version of [4,3,2,1]) {
  const candidate=join(directory,`settings-v${version}.json`);
  try {saved=JSON.parse(await readFile(candidate,'utf8'));existingPath=candidate;break;}catch(error){if(error.code!=='ENOENT')throw error;}
}
const settings=validateSettings(saved??types.defaultSettings);
const community=JSON.parse(await readFile(join(root,'data/weapons/community-profiles.json'),'utf8'));
for(const profile of community) {
  settings.overrides[profile.id]={...(settings.overrides[profile.id]??profile),sourceAcknowledged:true};
}
const validated=validateSettings(settings);
const bytes=JSON.stringify(validated,null,2);
if(Buffer.byteLength(bytes)>512000)throw new Error('Settings exceed the native size limit');
if(existingPath)await copyFile(existingPath,join(directory,`settings-before-source-acceptance-${Date.now()}.json`));
const target=join(directory,'settings-v4.json');
const temporary=join(directory,'settings-v4.source-acceptance.tmp');
await writeFile(temporary,bytes,'utf8');await rename(temporary,target);
console.log(JSON.stringify({acceptedCommunityProfiles:community.length,schemaVersion:validated.schemaVersion,localCalibrationsPreserved:true,parametersMarkedVerified:false}));
