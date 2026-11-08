# Vice City DE co-op: planas (2026-11-08)

Tikslas: tas pats modas, kas SA DE co-op, bet GTA Vice City – The Definitive Edition (Steam 1546990,
build 1.0.113.49697469). Vienas repo, viena `coopnet64.asi`, du žaidimai.

## Kaip veiks (žaidėjui)

1. `sa-coop.exe` (paleidiklis) randa abu žaidimus ir įdiegia modą į tą, kurį pasirenki (SA, VC arba abu).
2. Žaidime meniu atsiranda **co-op** įrašas (pagrindiniame ir pauzės meniu), F9 veikia visada.
3. Sesija: pavadinimas + slaptažodis per Steam vestibiulius (VC turi savo Steam app ID, todėl vestibiuliai
   atskiri nuo SA) arba IP kodas per paleidiklį.
4. Draugas tavo pasaulyje: vaikšto, bėga, vairuoja, ginklas, šūviai, vardas virš galvos, žymė žemėlapyje.
5. G: sėsti šalia draugo jo mašinoje. E: atgaivinti sužeistą draugą. F: iššokti iš draugo mašinos.
6. Bendras laikas, oras, ieškomumas, pjūviai, misijos tekstas ir tikslo žymės (šeimininkas veda istoriją).

## Architektūra

| Dalis | SA | VC | Bendra? |
| --- | --- | --- | --- |
| `proto/` (80 B būsena, įvykiai) | taip | taip | bendra, nekeičiama |
| `relay/` | taip | taip | bendra |
| `coopnet/` tinklas, Steam, F9 langas | taip | taip | bendra; žaidimas pagal exe vardą |
| `coopnet/` meniu kabliukas (ProcessEvent, GNames) | SA adresai | VC adresai | `game.rs` lentelė |
| `coopnet/` skriptų kabliukas (opcode lentelė) | SA | VC | `game.rs` lentelė; VC be CanPlayerStartMission |
| `coopnet/` išsaugojimai | SA aplankas | VC aplankas | pagal žaidimą |
| `game/CLEO/sa-coop.js` | taip | ne | |
| `game/CLEO/vc-coop.js` | ne | taip | naujas, VC komandos + atmintis |
| meniu `~mods/coop_P.pak` | GTASA_* šablonai | VC šablonai | MenuPatch su parametrais |
| `launcher/` | taip | taip | žaidimo pasirinkimas |

### Ko VC DE neturi (palyginus su SA) ir kuo pakeičiame

- WARP_CHAR_INTO_CAR_AS_PASSENGER, DOES_VEHICLE_EXIST: draugo kopija keleivio vietoje kuriama
  `Char.CreateAsPassenger`; žaidėjui G: draugo mašinos kopija sustabdoma (`Car.FreezePosition`) ir
  `SetObjEnterCarAsPassenger`, po to vėl sekama.
- Ketvirtainiai (quaternion), greitis, interjeras (area), kameros padėtis: skaitomi tiesiai iš atminties per
  objekto rodyklę (handle -> pool), adresai iš statinės analizės.
- Task.* (SA užduotys): VC turi tik „objective“ komandas (`SetObjRunToCoord`, `SetObjSprintToCoord`,
  `SetObjGotoCoordOnFoot`), jomis vaikščioja draugo kopija.
- Sužeistas: `Char.SetWaitState` / užšaldymas vietoje vietoj animacijos.

## Atvirkštinė inžinerija (ViceCity.exe 1.0.113.49697469)

1. UE: `UObject::ProcessEvent`, `GNames` (tas pats UE 4.26: lyginama su SanAndreas.exe baitais).
2. Skriptai: opcode handler lentelė, `CTheScripts::ScriptSpace`, aktyvių skriptų sąrašas, CRunningScript
   poslinkiai, `$ONMISSION` poslinkis, misijos valymo sąrašas.
3. Pasaulis: ped/vehicle pool, CPlaceable matrica, CPhysical greitis, entity area, `CGame::currArea`,
   kamera, `CWeather::NewWeatherType/ForcedWeatherType`, pinigai, pjūvio vardas ir būsena,
   `CMessages::BriefMessages`, `CRadar::ms_RadarTrace`, paskutinė įveikta misija.
4. Meniu: VC pak'e meniu šablonų vardai ir mygtukų klasės.

Kiekvienas adresas tikrinamas baitais paleidžiant (kitas build'as = funkcija išjungiama, žaidimas nelūžta).

## Tikrinimas

- `cargo test`, `cargo build --release`, `python -I tools/dll_test.py` (be žaidimo).
- Žaidime: VC paleidžiamas su CLEO Redux, `cleo_redux.log` ir `coopnet.log` be klaidų, robotas
  `tools/botas.py` kaip draugas.
- Kas nepatikrinta žaidime, pažymima README ir atmintyje.
