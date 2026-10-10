import os

build_dir = r'C:\Users/pavan/PROJECTS/Mayasaba/app/Mayasaba.App/x64/Release/Mayasaba.App'

all_files = []
for root, dirs, files in os.walk(build_dir):
    for f in sorted(files):
        full = os.path.join(root, f)
        rel = os.path.relpath(full, build_dir)
        if rel.endswith(('.pdb', '.lib', '.exp', '.log')):
            continue
        all_files.append(rel.replace('\\', '/'))
all_files.sort()
print(f"Total files: {len(all_files)}")

LCID = {
    'af-ZA': 1074, 'am-ET': 1118, 'ar-SA': 1025, 'az-Latn-AZ': 1068,
    'bg-BG': 1026, 'bn-IN': 1026, 'bs-Latn-BA': 3077, 'ca-ES': 1027,
    'ca-ES-VALENCIA': 1027, 'cs-CZ': 1029, 'cy-GB': 1091, 'da-DK': 1030,
    'de-DE': 1031, 'el-GR': 1032, 'en-GB': 2057, 'en-US': 1033,
    'es-ES': 1034, 'es-MX': 2058, 'et-EE': 1027, 'eu-ES': 1034,
    'fa-IR': 1065, 'fi-FI': 1035, 'fr-CA': 3084, 'fr-FR': 1036,
    'fy-NL': 1079, 'ga-IE': 1036, 'gd-GB': 1036,
    'gl-ES': 1110, 'gu-IN': 1079, 'he-IL': 1037, 'hi-IN': 1037,
    'hr-HR': 1050, 'hu-HU': 1038, 'hy-AM': 1050,
    'id-ID': 1033, 'is-IS': 1033, 'it-IT': 1040, 'ja-JP': 1041,
    'ka-GE': 1033, 'kk-KZ': 1033, 'kl-GL': 1033, 'kn-IN': 1033,
    'ko-KR': 1042, 'lb-LU': 1033, 'lt-LT': 1043, 'lv-LV': 1062,
    'mi-NZ': 1033, 'mk-MK': 1033, 'ml-IN': 1033, 'mn-MN': 1033,
    'ms-MY': 1033, 'nb-NO': 1035, 'ne-NP': 1033, 'nl-BE': 2057,
    'nl-NL': 1043, 'nn-NO': 1033, 'nso-ZA': 1033, 'or-IN': 1033,
    'pa-IN': 1033, 'pl-PL': 1045, 'pt-BR': 1046, 'pt-PT': 1036,
    'ro-RO': 1048, 'ru-RU': 1049, 'sd-Arabic-PK': 1033, 'si-LK': 1033,
    'sk-SK': 1051, 'sl-SI': 1054, 'sq-AL': 1033, 'sr-Cyrl-BA': 1033,
    'sr-Cyrl-CS': 1033, 'sr-Latn-BA': 1033, 'sv-SE': 1053, 'sw-KE': 1033,
    'ta-IN': 1033, 'te-IN': 1033, 'tg-Cyrl-TJ': 1033, 'th-TH': 1054,
    'tn-ZA': 1033, 'tr-TR': 1055, 'tt-RU': 1033, 'ug-CN': 1033,
    'uk-UA': 1058, 'ur-PK': 1056, 'uz-Latn-UZ': 1057, 'vi-VN': 1066,
    'wo-SN': 1033, 'xh-ZA': 1033, 'yo-NG': 1033, 'zh-CN': 2052,
    'zh-TW': 1028, 'zu-ZA': 1033,
}

# Pre-compute a lowercase-keyed version of LCID for case-insensitive lookup.
LCID_LO = {}
for _k, _v in LCID.items():
    LCID_LO[_k.lower()] = _v

def _lcid_for_mui_dir(dir_name):
    """Return the LCID for a .mui locale directory name like 'gd-gb', 'en-US'.
    Tries exact, lowercased, and title-cased lookups against the LCID map."""
    if dir_name in LCID:
        return LCID[dir_name]
    lo = dir_name.lower()
    if lo in LCID_LO:
        return LCID_LO[lo]
    return None

def make_guid(n):
    h = '%032x' % n
    return '{' + '-'.join([h[0:8], h[8:12], h[12:16], h[16:20], h[20:32]]) + '}'

dirs_set = set([''])
for rel in all_files:
    parts = rel.split('/')
    for i in range(len(parts)-1):
        dirs_set.add('/'.join(parts[:i+1]))

dir_tree = {}
for d in sorted(dirs_set, key=lambda d: (d.count('/'), d)):
    parts = d.split('/') if d else []
    node = dir_tree
    for p in parts:
        if p not in node:
            node[p] = {}
        node = node[p]

def pid(p):
    if not p: return 'MAYASABA_FOLDER'
    return 'DIR_' + p.replace('/', '_').replace('-', '_').replace('.', '_').replace(' ', '_')[:80]

def pname(p):
    if not p: return 'Mayasaba'
    return p.split('/')[-1]

wxs = []
wxs.append('<?xml version="1.0" encoding="UTF-8"?>')
wxs.append('<!--')
wxs.append('  Mayasaba.msi - WiX v6 source for the unpackaged Windows App SDK / WinUI 3 app.')
wxs.append('  Build: wix build packaging\\Mayasaba.wxs -d AppBinDir=app\\Mayasaba.App\\x64\\Release\\Mayasaba.App\\ -b app\\Mayasaba.App\\x64\\Release\\Mayasaba.App -out packaging\\release\\Mayasaba.msi')
wxs.append('-->')
wxs.append('<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">')
wxs.append('  <Package Name="MayasabaControlRoom"')
wxs.append('           Manufacturer="Mayasaba Project"')
wxs.append('           Version="1.0.0.0"')
wxs.append('           InstallerVersion="201"')
wxs.append('           Compressed="yes"')
wxs.append('           Scope="perUser"')
wxs.append('           Language="1033"')
wxs.append("           Codepage='65001'")
wxs.append('           ProductCode="{9750A5BF-D2AC-4C2D-B4E5-6FE45D8C2D3E}"')
wxs.append('           UpgradeCode="B2A1F7C9-9E58-4F6A-8B17-000000000001">')
wxs.append('    <MediaTemplate EmbedCab="yes" />')
wxs.append('    <!-- Scope="perUser" authors ALLUSERS="" and MSIINSTALLPERUSER=1; do not set them here. -->')
wxs.append('    <Property Id="MSIFASTINSTALL" Value="7" />')
wxs.append('    <Property Id="ARPHELPLINK" Value="https://github.com/pavanmayasaba/Mayasaba" />')
wxs.append('    <Property Id="ARPURLINFOABOUT" Value="https://github.com/pavanmayasaba/Mayasaba" />')
wxs.append('    <Property Id="ARPNOREPAIR" Value="1" />')
wxs.append('')
wxs.append('    <StandardDirectory Id="LocalAppDataFolder">')
wxs.append('      <Directory Id="MAYASABA_FOLDER" Name="Mayasaba">')

def emit(node, depth, path=''):
    r = []
    prefix = '  ' * depth
    for key in sorted(node.keys()):
        cp = path + '/' + key if path else key
        cid = pid(cp)
        cn = pname(cp)
        r.append('%s<Directory Id="%s" Name="%s">' % (prefix, cid, cn))
        r.extend(emit(node[key], depth+1, cp))
        r.append('%s</Directory>' % prefix)
    return r

wxs.extend(emit(dir_tree, 3))
wxs.append('      </Directory>')
wxs.append('    </StandardDirectory>')
wxs.append('')

# Group files by their destination directory so each directory gets its own
# component (File installs into its parent Component's directory).
# Root files (rel == "foo.dll") -> MAYASABA_FOLDER.
# "Chat/ChatPage.xaml" -> DIR_Chat, etc. Each component gets an HKCU registry
# keypath to satisfy ICE38. The exe file is the keypath-having File in root.
from collections import defaultdict

files_by_dir = defaultdict(list)
for f in all_files:
    d = '' if '/' not in f else f.rsplit('/', 1)[0]
    files_by_dir[d].append(f)
# stable, content-ordered output
ordered_dirs = list(dict(sorted(files_by_dir.items(), key=lambda kv: (-kv[0].count('/'), kv[0]))))
# Identify container-only directories (have subdirs but no direct files).
# These need their own component+RemoveFolder so ICE64 stops complaining.
container_only_dirs = sorted(
    [d for d in dirs_set - {''} if not files_by_dir.get(d)],
    key=lambda d: (-d.count('/'), d)
)


wxs.append('    <Component Id="AppFiles" Guid="%s" Directory="MAYASABA_FOLDER">' % make_guid(1000))
for f in files_by_dir.get('', []):
    fname = f.split('/')[-1]
    fid = 'F_' + f.replace('/', '_').replace('.', '_').replace('-', '_')[:50]
    wxs.append(r'      <File Id="%s" Source="$(var.AppBinDir)\%s" />' % (fid, f))
# Per-user install: every component's keypath must be an HKCU registry VALUE (ICE38).
wxs.append('      <RegistryKey Root="HKCU" Key="Software\\Mayasaba\\App">')
wxs.append('        <RegistryValue Name="Root" Type="integer" Value="1" KeyPath="yes" />')
wxs.append('      </RegistryKey>')
wxs.append('      <RemoveFolder Id="RF_MAYASABA_FOLDER" On="uninstall" />')
wxs.append('    </Component>')
wxs.append('')

dir_guid_counter = 1001
for d in ordered_dirs:
    if d == '':
        continue
    # .mui .mui locale dirs use LongName (emit() handles that) to prevent
    # WiX language inference. No Language attr needed here.
    wxs.append('    <Component Id="%s" Guid="%s" Directory="%s">' % (pid('COMP_' + d), make_guid(dir_guid_counter), pid(d)))
    dir_guid_counter += 1
    for f in files_by_dir[d]:
        fname = f.split('/')[-1]
        fid = 'F_' + f.replace('/', '_').replace('.', '_').replace('-', '_')[:50]
        wxs.append(r'      <File Id="%s" Source="$(var.AppBinDir)\%s" />' % (fid, f))
    # ICE38: HKCU registry VALUE keypath, unique per component.
    wxs.append('      <RegistryKey Root="HKCU" Key="Software\\Mayasaba\\App">')
    wxs.append('        <RegistryValue Name="%s" Type="integer" Value="1" KeyPath="yes" />' % pid('COMP_' + d))
    wxs.append('      </RegistryKey>')
    # ICE64: per-user directories are removed on uninstall when empty.
    wxs.append('      <RemoveFolder Id="RF_%s" On="uninstall" />' % pid('COMP_' + d))
    wxs.append('    </Component>')
    wxs.append('')

# Container-only components: directories with only subdirectories (e.g.
# Microsoft.UI.Xaml which holds language dirs but no direct files). Each
# needs a component + RemoveFolder so ICE64 is satisfied.
for d in container_only_dirs:
    wxs.append('    <Component Id="%s" Guid="%s" Directory="%s">' % (pid('COMP_' + d), make_guid(dir_guid_counter), pid(d)))
    dir_guid_counter += 1
    wxs.append('      <RemoveFolder Id="RF_%s" On="uninstall" />' % pid('COMP_' + d))
    wxs.append('      <RegistryKey Root="HKCU" Key="Software\\Mayasaba\\App">')
    wxs.append('        <RegistryValue Name="%s" Type="integer" Value="1" KeyPath="yes" />' % pid('COMP_' + d))
    wxs.append('      </RegistryKey>')
    wxs.append('    </Component>')
    wxs.append('')

wxs.append(r'    <Icon Id="AppIcon" SourceFile="$(var.AppBinDir)\assets/mayasaba.ico" />')
wxs.append('    <Property Id="ARPPRODUCTICON" Value="AppIcon" />')
wxs.append('')
wxs.append('    <StandardDirectory Id="ProgramMenuFolder">')
wxs.append('      <Directory Id="STARTMENU_FOLDER" Name="Mayasaba Control Room" />')
wxs.append('    </StandardDirectory>')
wxs.append('')
wxs.append('    <Component Id="StartMenuShortcut" Guid="%s" Directory="STARTMENU_FOLDER">' % make_guid(2000))
wxs.append('      <Shortcut Id="StartMenuShortcut"')
wxs.append('                Name="Mayasaba Control Room"')
wxs.append('                Description="Native Windows desktop control plane for coding agents"')
wxs.append('                Target="[MAYASABA_FOLDER]Mayasaba.App.exe"')
wxs.append('                WorkingDirectory="MAYASABA_FOLDER"')
wxs.append('                Icon="AppIcon" />')
wxs.append('      <RemoveFolder Id="STARTMENU_FOLDER" On="uninstall" />')
wxs.append('      <RegistryKey Root="HKCU" Key="Software\\Mayasaba\\Shortcut">')
wxs.append('        <RegistryValue Type="integer" Name="installed" Value="1" KeyPath="yes" />')
wxs.append('      </RegistryKey>')
wxs.append('    </Component>')
wxs.append('')
wxs.append('    <Feature Id="Complete" Level="1" Title="Mayasaba Control Room">')
wxs.append('      <ComponentRef Id="AppFiles" />')
wxs.append('      <ComponentRef Id="StartMenuShortcut" />')
for d in ordered_dirs:
    if d == '':
        continue
    wxs.append('      <ComponentRef Id="%s" />' % pid('COMP_' + d))
for d in container_only_dirs:
    wxs.append('      <ComponentRef Id="%s" />' % pid('COMP_' + d))
wxs.append('    </Feature>')

component_count = 2 + len([d for d in ordered_dirs if d]) + len(container_only_dirs)
wxs.append('')
wxs.append('    <MajorUpgrade DowngradeErrorMessage="A newer version of Mayasaba is already installed." />')
wxs.append('  </Package>')
wxs.append('</Wix>')

content = '\r\n'.join(wxs)
wxs_path = r'C:\Users/pavan/PROJECTS/Mayasaba\packaging/Mayasaba.wxs'
with open(wxs_path, 'w', newline='', encoding='utf-8') as f:
    f.write(content)

fc = len([l for l in wxs if '<File ' in l and 'Ref' not in l])
print(f'Generated {len(wxs)} lines, {fc} File entries, {component_count} Components')
