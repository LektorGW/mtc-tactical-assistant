# Profile uzbrojenia 0.2

Katalog obejmuje siedem lokalnych szablonów oraz 153 eksperymentalne profile pocisków dla 51 nazw uzbrojenia z publicznego kalkulatora MTC. `builtin.json` zachowuje szablony ze stanem `no_data`; `community-profiles.json` zawiera przypięte dane wraz z pochodzeniem. [Źródła, jednostki i licencja](community-sources.md).

## Schemat JSON i zgodność z v1

Profil nadal używa `schemaVersion: 1`; wersja aplikacji 0.2 nie wymaga zmiany numeru schematu profilu. Rust odrzuca nieznane pola i waliduje dane przed użyciem. Pola identyfikujące i opisowe to `id`, `name`, `category`, `gameVersion`, `status`, `source`, `calibrated` i `notes`. Parametry modelu: `speed`, `gravity`, `minElevationDeg`, `maxElevationDeg`, `dispersionSigma` oraz `salvoSize`. Brak znanej prędkości, grawitacji lub rozrzutu zapisuj jako JSON `null`.

W 0.2 dodano pola zgodne ze starszymi profilami:

| Pole | Znaczenie | Domyślna wartość przy braku pola |
|---|---|---|
| `unit` | `game_units` — własne spójne jednostki; `game_meters` — konwencja źródłowego kalkulatora | `game_units` |
| `sourceMetadata` | Struktura pochodzenia danych społecznościowych | `null` |
| `sourceAcknowledged` | Jawne potwierdzenie użycia danych dla tego konkretnego pocisku | `false` |

`sourceMetadata` zawiera `repository`, pełny 40-znakowy `commit`, `fetchedAt`, `license`, listę `urls` i numeryczny `experienceId`. Adresy muszą używać HTTPS; pochodzenie musi być kompletne. Walidacja struktury nie dowodzi, że parametry są aktualne. Commit i data importu nie są wersją gry.

Kategorie: `rocket_artillery`, `tube_artillery`, `tank`, `helicopter_gun`, `helicopter_rocket`, `guided_missile`, `anti_aircraft`, `direct_fire` (lokalne profile ognia bezpośredniego). Statusy: `no_data`, `experimental`, `beta`, `verified`. Lokalna kalibracja z UI ma status beta, bez automatycznej promocji do verified. Importowany katalog społecznościowy pozostaje experimental. Grupowanie zewnętrznych profili opiera się na nazwach; nie potwierdza mechaniki każdego wariantu uzbrojenia.

## Parametry i bramka obliczeń

Wartości muszą być skończone. Prędkość i grawitacja — dodatnie; σ rozrzutu — nieujemne; salwa — całkowita 1–1000; limity elewacji — od −89° do 89°, z dolnym nie większym niż górny. `calibrated: true` wymaga prędkości i grawitacji. Jednostka mapy, dystansu, wysokości oraz prędkości celu/stanowiska musi odpowiadać jednostce profilu.

Solwer dopuszcza:

- lokalny profil z `calibrated: true` oraz kompletnymi parametrami;
- nieskalibrowany profil z kompletnym `sourceMetadata`, statusem experimental/beta, prędkością, grawitacją i `sourceAcknowledged: true`.

Samo potwierdzenie bez parametrów lub pochodzenia nie odblokowuje modelu. Checkbox **Używaj tych eksperymentalnych danych dla wybranego pocisku** zapisuje potwierdzenie dla wybranego profilu; nie potwierdza aktualnej gry i nie zmienia stanu kalibracji. Regenerowany snapshot społeczności resetuje domyślne potwierdzenia. Wszystkie profile `guided_missile` pozostają zablokowane również po potwierdzeniu; deklarowanie ich lokalnej kalibracji balistycznej jest odrzucane.

## Co rzeczywiście modeluje aplikacja

`ballistics::solve` oblicza niski/wysoki tor, czas lotu i opcjonalny promień rozrzutu pojedynczego pocisku w stałej grawitacji, bez oporu i przyspieszania rakiet. Limity elewacji źródła społecznościowego są matematyczne; nie stanowią potwierdzonych ograniczeń mechanizmu pojazdu. Prognozę całej salwy liczy osobne `salvo_pattern` (sekcja „Rozrzut salwy” niżej); bez jawnych parametrów rozrzutu jest niedostępna.

Wyprzedzenie jest osobnym przybliżeniem przechwycenia celu w dwóch wymiarach przy stałej prędkości, **bez grawitacji**. Prędkość celu pochodzi z ręcznych danych lub opcjonalnego trackera świeżego widocznego markera. W UI można osobno włączyć dziedziczenie prędkości stanowiska; model używa wtedy ruchu względnego celu i własnego pojazdu. Prędkość własna jest ręczna albo pochodzi z markera w Automatic. Założenie dziedziczenia wymaga walidacji mechaniki gry; nie zmienia prostoliniowego modelu w pełny solwer ruchomego stanowiska z grawitacją.

Napęd, drag, naprowadzanie, przyspieszenie celu, pełny lot 3D i wysokość terenu nie są modelowane. Dostępność kategorii czołg/śmigłowiec/AA oznacza profil i jawny zakres wspólnego modelu, bez obietnicy kompletnych dedykowanych mechanik.

## Kalibracja, import i eksport

W **Profile i balistyka** wpisz zmierzoną prędkość, grawitację gry, źródło, opcjonalne σ i liczbę pocisków. **Zastosuj lokalną kalibrację** zapisuje profil beta. W źródle zachowaj amunicję, wersję gry, datę i opis pomiaru. Limity elewacji można edytować w JSON.

**Importuj profil JSON** waliduje plik przez Rust; **Eksportuj profil** zachowuje jednostkę, parametry, status i metadane źródła. Pola opcjonalne umożliwiają odczyt starszych profili v1. To niezależne od ustawień aplikacji, które w 0.2 używają pliku `settings-v2.json` i migracji ze starszego zapisu. [Procedura kalibracji](calibration.md).

## Rozrzut salwy (`salvo`) i zgodność walidacji

Opcjonalne pole `salvo` (schemat profilu nadal v1; brak pola = brak danych):

| Pole | Znaczenie |
| --- | --- |
| `unit` | Jednostka profilu |
| `angularSigmaMrad` | 1σ rozrzutu kątowego startu na każdą z dwóch osi poprzecznych, mrad (0–500) |
| `speedSigma` | 1σ rozrzutu prędkości początkowej, jednostka/s |
| `rippleInterval` | Odstęp startów w salwie, s — tylko czasy trafień |
| `calibrated`, `status`, `source` | `calibrated: true` wymaga `beta`; status inny niż `no_data` wymaga źródła i co najmniej jednej składowej σ |

Brak jednej składowej jest raportowany w `missingComponents` jako nieuwzględniony — elipsa może być zaniżona. Liczba pocisków to `salvoSize` profilu albo jawnie podana w zapytaniu liczba zaobserwowana w grze.

Jedyny wpis spoza snapshotu grand-hawk: `community-bm_21_grad-1` (BM-21 Grad / 9M22/M21 HE-Frag) ma `angularSigmaMrad: 1.75`, `status: experimental`, z [multicrew.wiki](https://multicrew.wiki/md/mtc/shells/122mm-grad-9m22m21-he-frag.md) („Angular dispersion (1σ): 1.75 mrad”, odczyt 2026-10-15; strona bez daty, wersji gry i licencji). Wartość wpisano ręcznie w `data/weapons/community/curated-parameters.json`; `scripts/sync-community-data.py` nakłada ten plik po każdym imporcie i wymaga statusu experimental ze źródłem. Interpretacja „na oś poprzeczną” jest założeniem aplikacji. Profil LUAZ-672 z tym samym pociskiem nie dostał tej wartości — strona nie opisuje wyrzutni. Nie importujemy hurtowo danych wiki.

Walidacja TypeScript (`validation.ts`, `salvo.ts`) odpowiada teraz Rust: odrzuca nieznane pola profilu (jak `deny_unknown_fields`), wymaga kompletnego `sourceMetadata` (`https://`, `fetchedAt`, numeryczne `experienceId`, niepusta lista adresów), traktuje brak `speed`/`gravity`/`dispersionSigma` jak `null` i waliduje `salvo`. Rust odrzuca z kolei model drag/rocket w profilu `guided_missile`, tak jak UI.

## Profile ognia bezpośredniego (`direct_fire`)

Kategoria `direct_fire` służy do lokalnych profili broni ręcznej i dział ognia bezpośredniego (np. RPG-7V2 / PG-7VR, SPG-9 / PG-9, PTRS-41). Profil tworzy się w zakładce „Broń bezpośrednia / śmigłowiec”: prędkość odczytuje się przyciskiem **Odczytaj prędkość z ekranu Loadout** (`read_loadout_specs`) albo wpisuje ręcznie. Profil ma `status: experimental`, `unit: game_meters`, źródło „odczytane z UI gry <data> (Loadout…)” lub „wpisane ręcznie <data>” oraz pole:

| `observedSpecs` | Znaczenie |
| --- | --- |
| `screen` | `loadout` albo `manual` |
| `observedAt`, `weaponName?` | Czas odczytu, opcjonalna nazwa |
| `muzzleVelocity` | Wartość jak w UI; musi być równa `speed`, dopóki profil nie jest lokalnie skalibrowany |
| `velocityUnitAssumption`, `gravitySource` | Jawne założenia: „m/s” = m gry/s; 17,64 m gry/s² z kalkulatora |

Nieskalibrowany profil liczy się tylko z kompletnym `sourceMetadata` **albo** `observedSpecs` oraz `sourceAcknowledged: true` (checkbox założeń przy tworzeniu).

Opcjonalne `reticle` to mapa kresek celownika: `{marks: [{mark, range}], status: unverified|experimental|beta, source}`, z rosnącymi dystansami i 1–32 kreskami. Przycisk wstawia założenie PGO-7 „kreska N ≈ N×100 m” ze statusem **unverified**; gracz może je edytować. Strzały kontrolne zgodne z modelem vacuum zapisują lokalną kalibrację beta z efektywną prędkością; pozostałe trafiają tylko do logu kalibracji.

Walidacja Rust i TS (`validation.ts`, `direct-fire.ts`) przyjmuje te same pola i odrzuca nieznane (`deny_unknown_fields`). Moduł nie wykrywa ani nie śledzi celów i nie rysuje znaczników na celach — podaje tylko liczby.
