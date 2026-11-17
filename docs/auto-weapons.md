# Rozpoznawanie uzbrojenia, kalibracja i wyprzedzenie 3D

Biblioteki `mtc-profiles` i `mtc-ballistics` obsługują jawne etykiety OCR, lokalne pomiary jednego pocisku oraz ruch celu w trzech wymiarach. Zachowano dotychczasowe API `solve()` i `intercept()`. Stary `intercept()` pozostaje osobnym przybliżeniem ruchu prostoliniowego bez grawitacji; nie jest automatycznym fallbackiem nowego solwera.

## Jawna nazwa pojazdu i amunicji

`resolve_weapon_labels(profiles, vehicle, ammunition)` przyjmuje opcjonalne `OcrWeaponLabel {text, confidence}`. Confidence musi pochodzić z obserwacji OCR. Resolver nie tworzy pewności pomiaru i nie analizuje sylwetki, koloru pojazdu ani domniemanej fizyki.

Normalizacja usuwa znaki rozdzielające i różnice wielkości liter. Nie zamienia `O` na `0`, `I` na `1`, nie korzysta z podobieństwa podciągów i nie usuwa znaków diakrytycznych. Kolizja nazw po normalizacji pozostaje niejednoznaczna.

| Etykiety | Wynik |
| --- | --- |
| Dokładna pełna nazwa `pojazd / amunicja` | `matched`, jeżeli pasuje jeden profil |
| Dokładny pojazd i dokładna amunicja | Zawężenie do wspólnych kandydatów; `matched` tylko przy jednym |
| Sam pojazd | Lista amunicji i `ambiguous`, także przy jednej amunicji w obecnym katalogu — jej użycia nie zaobserwowano |
| Sama amunicja | Wszyscy pasujący kandydaci; wspólny pocisk kilku pojazdów pozostaje `ambiguous` |
| Sprzeczne etykiety, brak etykiet lub nieznana nazwa | `no_match`; brak cichego wyboru pierwszego profilu |

`WeaponResolution` zawiera `status`, opcjonalny `profileId`, `candidates`, `confidence` oraz `evidence`. Przy dopasowaniu obu etykiet confidence jest minimum ich obserwowanych wartości; przy braku dopasowania wynosi zero. `matched` oznacza zgodność tekstu z katalogiem, bez potwierdzenia kalibracji i mechaniki. Automatyczny wybór nadal wymaga osobnej kontroli aktualności, stabilności i jakości OCR przez warstwę analizy.

`WeaponIdentity {profileId, vehicle, projectile, category}` rozdziela nazwę na pierwszym dokładnym separatorze ` / `. Nazwy pocisków ze zwykłym `/`, np. `9M22/M21 HE-Frag`, pozostają kompletne. Nazwy 153 profili wygenerowano z `gun.name` i `projectile.name` przypiętego [repozytorium MIT](https://github.com/grand-hawk/artillery-calculator/tree/39a7af7689524e57ca2326f3b998cf123df95426); test porównuje wszystkie rekordy `importedGuns.json` z ich strukturą. Snapshot nie został zmieniony. Szczegóły pochodzenia i licencji: `docs/community-sources.md`.

## Dopasowanie modelu vacuum z pomiarów

`fit_vacuum(profile, samples, options)` dopasowuje jednocześnie prędkość początkową `v` i dodatnią grawitację gry `g`, bez oporu i napędu. Próbka `CalibrationSample` wymaga:

- `range`: **poziomy** dystans celu od wylotu, bez doliczania różnicy wysokości;
- `elevationDeg`: zaobserwowany kąt wylotu;
- `flightTime`: zmierzony czas od opuszczenia wylotu do trafienia;
- `heightDelta`: wysokość trafienia minus wysokość wylotu, z dodatnią osią w górę;
- `unit` i `source`: zgodne jednostki gry oraz jawna identyfikacja pomiaru.

Nie używaj odliczania rundy jako czasu lotu. Nie podstawiaj automatycznie zerowej różnicy wysokości, jeśli jej nie zaobserwowano. Wszystkie próbki muszą dotyczyć tej samej amunicji, mechaniki i wersji gry.

Układ równań dla każdej próbki:

```text
range       = v × cos(elevation) × flightTime
heightDelta = v × sin(elevation) × flightTime − g × flightTime² / 2
```

`CalibrationOptions` zawiera `unit`, `maxResidualUnits`, `maxRelativeResidual`, `minSamples`, `source` oraz opcjonalne `gameVersion`. Nie ma domyślnych parametrów pocisku ani domyślnych tolerancji pomiaru; użytkownik określa tolerancje we własnych jednostkach. Wymagane są co najmniej trzy próbki i różne warunki czasu lub elewacji. Maksimum wynosi 1000 próbek jako ograniczenie wielkości danych.

Solwer używa najmniejszych kwadratów dla dwóch parametrów i sprawdza uwarunkowanie macierzy po normalizacji kolumn. Odrzuca dane niefinitywne, przepełnienie, niezgodne jednostki, brak źródła, kąty poza profilem, nieidentyfikowalne parametry oraz niedodatnie wyniki `v/g`. Maksymalna reszta absolutna **i** względna muszą spełniać podane limity. Nie usuwamy odstających obserwacji po cichu.

`CalibrationFit` zwraca `fittedProfile`, RMS, maksimum obu rodzajów reszty, `conditionNumber`, obserwowany zakres dystansów i reszty z identyfikacją źródeł. Zachowaj cały raport wraz z próbkami; profil zawiera skrócone pochodzenie lokalnej kalibracji. Wynik ma `calibrated=true` i `status=beta`, bez automatycznej zmiany `sourceAcknowledged`. Dotychczasowe `sourceMetadata` pozostaje historią użytego katalogu. Podane `gameVersion` zastępuje starą etykietę wersji profilu.

Dobry fit nie dowodzi braku oporu lub napędu poza obserwowanym zakresem, nie kalibruje rozrzutu i nie potwierdza pojemności salwy. Profile naprowadzane otrzymują błąd `lack_model`; ich mechaniki nie dopasowujemy fikcyjnym modelem parabolicznym.

## Przechwycenie balistyczne w 3D

`solve_intercept_3d(profile, input)` wymaga `InterceptInput3d`:

- `relative`: położenie celu względem wylotu `[wschód, północ, góra]`;
- `targetVelocity` i `ownVelocity`: pełne prędkości w tych samych osiach, w jednostkach/s;
- `inheritOwnVelocity`: świadomy wybór, czy pocisk dziedziczy prędkość stanowiska;
- `unit` i `maxTime`: jednostka zgodna z profilem oraz maksymalny horyzont lotu/ruchu.

Brak składowej wysokości lub pionowej prędkości blokuje pełną predykcję 3D. Biblioteka nie odtwarza ich z minimapy. `inheritOwnVelocity=null` jest dopuszczalne tylko przy zerowej prędkości własnej, gdy wybór nie zmienia obliczeń. Niezerowa prędkość stanowiska wymaga jawnego modelu dziedziczenia; sam ruch śmigłowca nie stanowi dowodu, że silnik gry dodaje tę prędkość pociskowi.

Solwer rozwiązuje:

```text
w = targetVelocity − inheritedOwnVelocity
|relative + w × t + [0, 0, g × t² / 2]|² = v² × t²
0 < t ≤ maxTime
```

Pierwiastki równania czwartego stopnia są izolowane przez jego ekstrema. Uwzględnia to styczne rozwiązanie maksymalnego zasięgu, które pomijałoby zwykłe szukanie zmian znaku na siatce czasu. Każdy wynik przechodzi limit horyzontu, limit elewacji oraz niezależne odtworzenie pozycji pocisku i celu przy zadanej prędkości początkowej.

`InterceptSolution3d` zwraca uporządkowane według czasu `arcs`, jednostkę i założenia. Każdy łuk zawiera czas, elewację, azymut, `launchVelocity`, `worldInitialVelocity`, `targetPositionAtImpact`, `lead`, `aimPoint` i `missDistance`. `lead` to ruch względny `w×t`, natomiast `aimPoint` zawiera także kompensację grawitacji. `targetPositionAtImpact` odnosi się do początkowej pozycji wylotu i rzeczywistej prędkości celu. Azymut ma północ 0°, wschód 90°.

Profile muszą przejść istniejącą bramkę kalibracji lub jawnie zaakceptowanego źródła community. Brak trajektorii nie skutkuje pokazaniem przybliżenia 2D. Założenie stałej prędkości celu i wybrany horyzont są jawne; solwer nie przewiduje przyspieszania, zakrętów ani automatycznego naprowadzania rakiet.

## Zakres kategorii i weryfikacja

Model vacuum obsługuje pociski lufowe i te rakiety, których mechanika została lokalnie sprawdzona jako zgodna z takim modelem. Nazwa kategorii nie ustawia współczynników. Nie dodano domyślnych współczynników oporu, napędu ani sterowania — przypięte źródło community ich nie podaje, a rzeczywistych pomiarów użytkownika jeszcze nie dostarczono.

Testy deterministyczne korzystają z **jawnie syntetycznych danych**, które nie są profilami MTC. Obejmują wszystkie 153 pełne nazwy, niejednoznaczność amunicji, zgodność ze źródłowym JSON, odtworzenie kalibracji `v/g`, szum i odstające pomiary, błędne jednostki, brak danych, ruch celu/stanowiska 3D, dwa łuki, styczny maksymalny zasięg, uciekający cel, limit elewacji/horyzontu i przepełnienie.

Sprawdzone polecenia z katalogu głównego:

```powershell
cargo fmt -p mtc-profiles -p mtc-ballistics
cargo clippy -p mtc-profiles -p mtc-ballistics --all-targets -- -D warnings
cargo test -p mtc-profiles -p mtc-ballistics
```

Wynik: 33 testy przeszły, formatowanie i Clippy bez ostrzeżeń. W tym etapie nie uruchamiano interfejsu, Computer Use, fizycznych klawiszy ani nagrywania. Integracja native/UI i przebudowanie wydania pozostają osobnym etapem.


## Wersja 0.4

Formularz **Opór i napęd pocisku** obsługuje jawnie wprowadzone parametry lokalnej kalibracji beta. Dostępne są trajektoria RK4 i statyczne rozwiązanie celowania dla drag/rocket; brak współczynników, niezgodne jednostki oraz ruch celu/stanowiska blokują wynik tego modelu. Starsze wyprzedzenie 2D i przechwycenie 3D nie mogą ignorować aktywnego oporu lub napędu. [Model i założenia](projectile-dynamics.md), [aktualny raport integracji](test-report-v0.5.0.md).
