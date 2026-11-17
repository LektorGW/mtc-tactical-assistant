# Numeryczne modele pocisku z pomiarów w grze

Moduły `mtc-profiles` i `mtc-ballistics` obsługują opcjonalny model oporu i napędu rakietowego. To modele fenomenologiczne do lokalnych obserwacji gry. Nie potwierdzają fizyki obecnego Multicrew Tank Combat. Nie zawierają parametrów rzeczywistych systemów uzbrojenia, śledzenia naprowadzanego pocisku ani automatyzacji sterowania.

Snapshot społeczności pozostaje przypięty do tego samego commitu. Żaden z 153 pocisków nie otrzymał współczynników oporu lub napędu. W importowanym źródle nie ma danych wystarczających do ich wyznaczenia. Nazwa pojazdu, wygląd kadłuba i kategoria uzbrojenia nie ustawiają tych współczynników. Rozpoznanie nazwy oraz zaakceptowanie źródła community nie oznaczają kalibracji nowego modelu.

## Schemat i jednostki

`WeaponProfile` zachowuje `schemaVersion: 1`. Opcjonalne `dynamics` jest domyślnie nieobecne i nie jest wtedy zapisywane przez serde. Dotychczasowe profile korzystają z wcześniejszego modelu vacuum. `dynamics.model` może mieć wartość `vacuum`, `drag` albo `rocket`.

| Pole dynamics | Znaczenie | Jednostka |
| --- | --- | --- |
| `unit` | Ta sama jednostka co profil, odległość i wysokość | `game_units` lub `game_meters` |
| `linearDrag` | Składnik przyspieszenia `-k1 * velocity` | `1/s` |
| `quadraticDrag` | Składnik przyspieszenia `-k2 * speed * velocity` | `1/unit` |
| `rocketAcceleration` | Stałe przyspieszenie podczas pracy napędu | `unit/s²` |
| `burnTime` | Czas pracy napędu od startu | sekundy |
| `thrustDirectionFixedLaunch` | Potwierdzenie przyjętego stałego kierunku napędu | wymagane `true` dla rocket |
| `calibrated` | Jawne potwierdzenie lokalnego przygotowania parametrów | domyślnie `false` |
| `status` | Stan modelu | `no_data`, `experimental`, `beta`; domyślnie `no_data` |
| `source` | Opis lokalnych pomiarów, wersji gry i sposobu dopasowania | tekst, maks. 2048 bajtów |

`unit` oznacza jednostkę gry. `game_meters` nie oznacza parametrów rzeczywistego uzbrojenia. Przeliczenie skali wymaga zgodności ze źródłem i mapą. Samodzielna zmiana nazwy jednostki nie przelicza współczynników: dla zmiany skali długości o czynnik `s`, prędkość, grawitacja i przyspieszenie mnożą się przez `s`, a `quadraticDrag` dzieli się przez `s`; `linearDrag` i czas nie zmieniają się.

Model drag wymaga jawnych `linearDrag` oraz `quadraticDrag`, nawet jeżeli któryś pomierzony składnik wynosi zero. Null oznacza brak danych. Rocket dodatkowo wymaga `rocketAcceleration >= 0`, skończonego `burnTime > 0` i `thrustDirectionFixedLaunch: true`. Ujemne lub nieskończone współczynniki są odrzucane. Wybranie vacuum z wpisanymi współczynnikami albo drag z wpisanym napędem jest błędem, ponieważ te parametry byłyby ignorowane.

Przykładowy fragment JSON do testu syntetycznego, który **nie jest kalibracją MTC**:

```json
{
  "dynamics": {
    "model": "rocket",
    "unit": "game_units",
    "linearDrag": 0,
    "quadraticDrag": 0,
    "rocketAcceleration": 20,
    "burnTime": 1.27,
    "thrustDirectionFixedLaunch": true,
    "calibrated": true,
    "status": "beta",
    "source": "TEST SYNTHETIC: kontrola RK4; brak pomiarów obecnego MTC"
  }
}
```

To wyłącznie fragment. Cały profil musi także mieć poprawne źródło, wersję, jednostkę, prędkość, grawitację i limity elewacji. Dla modelu non-vacuum `can_calculate()` wymaga jednocześnie `profile.calibrated == true`, `profile.status == beta`, `dynamics.calibrated == true`, `dynamics.status == beta`, kompletu parametrów i niepustego `dynamics.source`. `sourceAcknowledged` ze snapshotu nie omija tych warunków. Profile naprowadzane pozostają zablokowane niezależnie od pól dynamics.

## Równanie i założenia

Pozycja i prędkość mają trzy składowe: wschód, północ, góra. Stanowisko jest nieruchome, początek trajektorii znajduje się w wylocie, a początkowa prędkość ma długość `profile.speed`. Azymut 0° oznacza północ, 90° wschód. Elewacja musi mieścić się w limitach profilu.

Model integruje:

```text
d(position)/dt = velocity
d(velocity)/dt = [0, 0, -gravity]
                 - linearDrag * velocity
                 - quadraticDrag * |velocity| * velocity
                 + rocketAcceleration * initialLaunchDirection (tylko t < burnTime)
```

Napęd utrzymuje kierunek startowy. Nie obraca się razem z późniejszą prędkością, nie podąża za celem i nie modeluje naprowadzania. Wynik jawnie wymienia to założenie. Nie ma wiatru, siły nośnej, zmian masy, kolizji z terenem ani interpolowanych danych wysokości. Ujemna wysokość nie zatrzymuje symulacji, ponieważ poziom stanowiska nie jest rozpoznanym gruntem w każdym punkcie lotu.

Integrator RK4 skraca krok przy sztywnym oporze lub dużej zmianie prędkości. Dzieli krok dokładnie przy końcu pracy silnika, aby nie mieszać aktywnego i wyłączonego napędu w jednym kroku. Nie ogranicza ani nie zastępuje zadeklarowanych współczynników. Parametry, których nie można wiarygodnie integrować w zadanym budżecie, powodują błąd.

## Publiczne API Rust i JSON

W `mtc_profiles` dostępne są `DynamicsModel`, `ProjectileDynamics`, `WeaponProfile::dynamics_model()` oraz `WeaponProfile::require_vacuum()`.

`mtc_ballistics::simulate(&WeaponProfile, &SimulationInput)` zwraca `TrajectorySimulation`:

```text
SimulationInput {
  elevationDeg, azimuthDeg, unit, duration,
  options: { stepSeconds, maxSteps }
}
TrajectorySimulation {
  points: [{ time, position: [east,north,up], velocity: [east,north,up] }],
  unit, stepsUsed, assumptions
}
```

`mtc_ballistics::solve_dynamic(&WeaponProfile, &DynamicSolveInput)` wyszukuje trajektorie do **statycznego** celu:

```text
DynamicSolveInput {
  distance, heightDelta, azimuthDeg, unit,
  options: { maxTime, stepSeconds, maxWorkSteps,
             maxEvaluations, angleSamples, hitTolerance }
}
DynamicSolution {
  arcs: [{ elevationDeg, azimuthDeg, flightTime,
           impactPosition: [east,north,up], missDistance, highArc }],
  unit, minimumResidualUnits, stepsUsed, evaluations, assumptions
}
```

Wszystkie nazwy JSON są camelCase; Rust używa snake_case. `options` można pominąć w JSON, aby zastosować Default opcji numerycznych. To ustawienia obliczeń, nie współczynniki fizyki:

| Opcje | Default |
| --- | --- |
| SimulationOptions | krok 0.02 s, maks. 50 000 kroków |
| DynamicSolveOptions | horyzont 60 s, krok 0.02 s, maks. 1 000 000 kroków pracy, 512 prób, 65 kątów, tolerancja 0.05 jednostki gry |

Czas musi mieścić się od 0.000001 do 600 s; krok od 0.00001 do 0.1 s. Symulacja dopuszcza maks. 100 000 kroków, solver maks. 5 000 000 kroków pracy, 1024 próby i od 3 do 181 kątów. Tolerancja musi być dodatnia, nie większa niż 1 jednostka i mniejsza niż 1% odległości. Dodatkowe numeryczne granice wynoszą 10⁹ jednostek/s, 10¹² jednostek/s² i 10¹² jednostek położenia. Nie są to granice mechaniki gry. Przekroczenie budżetu lub granic zwraca błąd, bez częściowego rozwiązania.

## Wyszukiwanie i ograniczenia wyniku

Solver skanuje limity elewacji profilu. Dla każdej próby integruje lot do zadanej odległości albo końca horyzontu. Przejście przez odległość celu lokalizuje ponownymi podkrokami RK4. Wyszukuje zmianę znaku błędu wysokości i doprecyzowuje lokalne minima wartości bezwzględnej błędu. Umożliwia to wykrycie stycznej trajektorii przy maksymalnym zasięgu, dla której znak się nie zmienia.

Korzeń jest zwracany tylko wtedy, gdy odtworzone położenie pocisku ma `missDistance <= hitTolerance`. Korzenie nierozróżnialne w zadanej tolerancji przestrzennej są łączone. `highArc` oznacza wyższy z co najmniej dwóch odnalezionych korzeni; nie jest analityczną klasyfikacją wszystkich możliwych torów. `minimumResidualUnits` to najmniejszy zaobserwowany bezwzględny błąd wysokości przy odległości celu. Przy braku trafienia błąd zawiera ten residual, jeżeli dało się go obliczyć. Najbliższa nietrafiająca próba nie jest podawana jako rozwiązanie.

Skończony skan nie gwarantuje wszystkich korzeni dowolnego empirycznego modelu. Brak rozwiązania nie dowodzi, że strzał w grze jest niemożliwy. Mała tolerancja numeryczna nie oznacza takiej samej celności w MTC; model wymaga porównania z niezależnymi strzałami poza próbkami przygotowującymi parametry.

`solve`, `fit_vacuum` i `solve_intercept_3d` odrzucają aktywny model drag/rocket. Ruchomy cel dla drag/rocket obsługuje osobne `solve_dynamic_intercept` (status beta, opis niżej); nie wolno zastąpić go wynikiem vacuum. Starsza funkcja `intercept(relative, velocity, speed)` nie przyjmuje profilu; warstwa wywołująca musi blokować jej użycie dla non-vacuum. Guided nadal zwraca brak modelu.

Nie dodano automatycznego dopasowania współczynników dynamics. Próbki wystarczające do dopasowania dwóch niewiadomych vacuum nie dowodzą identyfikowalności oporu, napędu, czasu pracy i prędkości jednocześnie. Parametry muszą pochodzić z jawnych lokalnych pomiarów i porównań, z zapisanym źródłem. Testy syntetyczne opisane poniżej nie dostarczają kalibracji gry.

## Weryfikacja implementacji

Wykonane polecenia:

```powershell
cargo fmt -p mtc-profiles -p mtc-ballistics
cargo test -p mtc-profiles -p mtc-ballistics
cargo clippy -p mtc-profiles -p mtc-ballistics --all-targets -- -D warnings
```

Wynik: 19 testów profili i 32 testy balistyki, wszystkie poprawne; clippy bez ostrzeżeń. Nowe testy porównują każdy punkt 3D z analitycznym vacuum, końcowy stan z rozwiązaniem oporu liniowego, ubytek energii przy oporze kwadratowym i napęd kończący pracę po 1.27 s pomiędzy krokami. Strzały znalezione numerycznie są ponownie odtwarzane niezależną symulacją. Sprawdzono oba korzenie vacuum, korzeń styczny, zmianę toru przez opór oraz osiągalność z jawnie zadeklarowanym syntetycznym napędem. Testy sprawdzają też brak parametrów, jednostki, źródło/status, moc, czas, budżety, stałą i ograniczoną elewację oraz odmowę podania wyniku vacuum lub guided.

Implementacja nie zmienia żadnego parametru snapshotu ani nie uruchamia gry. Testy tego etapu dotyczą bibliotek Rust; integracja Windows, edytor parametrów i testy finalnego `.exe` należą do kolejnego przebiegu aplikacji.

## Wersja 0.4.x — audyt, przechwycenie i salwa

**Poprawka integratora.** Krok RK4 jest teraz dokładnie przycinany do końca pracy napędu i końca horyzontu (`next_time` = granica, bez kumulacji `time += h`). Wcześniej czas zatrzymywał się o ulp przed granicą i solver zgłaszał „krok zbyt mały” dla zwykłych danych, np. `burnTime` 0.1/2.9/3.3/7.7 s przy kroku 0.01–0.02 s (8 z 56 kombinacji w teście `burnout_and_horizon_rounding_never_demand_a_sliver_step`).

**Przechwycenie ruchomego celu z oporem/napędem (beta).** `mtc_ballistics::solve_dynamic_intercept(&profile, &DynamicInterceptInput)`, komenda Tauri `dynamic_intercept`:

```text
DynamicInterceptInput { relative:[e,n,u], targetVelocity:[3], ownVelocity:[3],
  inheritOwnVelocity: bool|null, unit, maxTime,
  options: { stepSeconds 0.02, maxWorkSteps 3 000 000, maxEvaluations 512,
             angleSamples 65, hitTolerance 0.05, maxIterations 30 } }
DynamicInterceptSolution { arcs:[{ elevationDeg, azimuthDeg, flightTime, launchVelocity,
  worldInitialVelocity, targetPositionAtImpact, projectilePositionAtImpact, lead,
  missDistance, iterations, highArc }], branches:[{ seedElevationDeg, seedFlightTime,
  converged, iterations, finalMiss, message }], status, unit, stepsUsed, assumptions }
```

Start: statyczny `solve_dynamic` do bieżącej pozycji celu (połowa budżetu kroków). Następnie tłumiony Newton na (elewacja, azymut, czas lotu): residuum = położenie pocisku z RK4 w chwili T minus położenie celu o stałej prędkości; Jacobian z różnic skończonych, pochodna po T analityczna; ograniczenie kroku i przeszukiwanie liniowe. Wynik tylko przy `missDistance ≤ hitTolerance`, elewacji w limitach profilu i `T ≤ maxTime`; każda gałąź raportuje zbieżność. Opór działa względem nieruchomego powietrza w układzie świata, więc dziedziczona prędkość stanowiska jest symulowana, a nie odejmowana. Napęd utrzymuje kierunek lufy. Guided pozostaje zablokowane. Dla vacuum wynik zgadza się z analitycznym `solve_intercept_3d` (test). `calculate` dla drag/rocket z ruchem 3D zachowuje teraz azymut i ostrzeżenie zamiast zwracać błąd całego obliczenia.

**Prognoza salwy.** `mtc_ballistics::salvo_pattern(&profile, &SalvoInput)`, komenda `salvo_pattern`. Rozrzut startowy z `profile.salvo` (σ kątowe w mrad na każdą oś poprzeczną, σ prędkości) jest propagowany przez ten sam model (vacuum/drag/rocket): różnice centralne o kroku 1σ, poprawka drugiego rzędu środka, przecięcie płaszczyzny wysokości celu najbliższe nominalnemu czasowi lotu. Alternatywnie izotropowe `dispersionSigma` z kalibracji; oba naraz są odrzucane jako niejednoznaczne. Bez nich — błąd „Brak danych rozrzutu”. Wynik: elipsa 95% pojedynczego pocisku, obwiednia wszystkich N (95%), elipsa środka salwy (Σ/N), σ wzdłuż/w poprzek, wskaźnik nieliniowości, opcjonalne pokrycie dla podanego promienia efektu i obszaru (deterministyczne próbki quasi-losowe) oraz czasy trafień przy `rippleInterval`. Założenia: nieruchomy cel i wyrzutnia, niezależne błędy pocisków, brak wspólnego błędu celowania i wiatru.

**Weryfikacja.** `cargo test -p mtc-profiles -p mtc-ballistics`: 24 + 48 testów; Clippy `-D warnings` bez uwag. Zgodność z kalkulatorem grand-hawk: 158 łuków z 7 profili katalogu (BM-21, M270 M26A1 High, D-30 3OF56 High, Mortar Low, 2B9 Low, 12-Pounder, M109 M107 Low), dystanse 150–6000 m gry, wysokości −40/0/35: maks. różnica elewacji 0°, czasu lotu 1.1·10⁻¹⁴ względnie; brak rozwiązania tylko tam, gdzie źródło zwraca NaN. Różnica świadoma: interfejs źródła liczy czas lotu z odległości skośnej 3D (`useDistanceWithHeight`), co przy różnicy wysokości daje kilka ms błędu (np. BM-21, 1500 m, +35 m: 9.5515 s zamiast 9.5489 s). Aplikacja używa odległości poziomej, zgodnej z równaniem toru.

## Ogień bezpośredni i śmigłowiec (rakiety niekierowane)

**Granica funkcji.** Moduł podaje graczowi wyłącznie liczby i tabele. Aplikacja **nie wykrywa ani nie śledzi przeciwników na ekranie, nie rysuje znaczników wyprzedzenia na celach** i nie steruje bronią ani pojazdem. Dystans pochodzi z presetu, z pola RANGE panelu broni gry (gdy jest odczytane) albo z wpisu gracza, np. z dystansu „184 meters” pokazanego przez grę.

Wszystkie funkcje używają modelu profilu: vacuum, chyba że profil ma lokalnie skalibrowany opór/napęd. Bez współczynników wynik ma ostrzeżenie „model uproszczony… wymaga strzałów kontrolnych” — dotyczy zwłaszcza PG-7VR i PG-9, które mogą mieć napęd rakietowy lub opór. Grawitacja 17,64 m gry/s² pochodzi z kalkulatora społeczności. Nie wiadomo, czy dotyczy broni ręcznej ani czy „m/s” z ekranu Loadout to metry gry/s; to jawne założenia, a status pozostaje experimental.

| Funkcja Rust / komenda Tauri | Wynik |
| --- | --- |
| `range_card` | Wiersze `{range, timeOfFlight, dropBelowLineOfSight, velocityAtRange, elevationMrad/Deg, holdOverMrad/Deg, reachable}`. Cel na wysokości linii celowania, opcjonalnie `sightHeight` i `zeroRange`. |
| `lead_estimate` (`lead`) | Wyprzedzenie pierwszego rzędu dla podanego czasu lotu. |
| `lead_iterated` | Wyprzedzenie z czasem lotu przeliczanym dla przewidywanego dystansu (iteracja punktu stałego). Kurs względny: 0° oddala się, 90° w prawo, 180° zbliża się, 270° w lewo. |
| `quick_aim` | Jedna linia trybu szybkiego. Zasięg strzału bezpośredniego jest liczony z toru dla progu \|opad\| podanego przez gracza (domyślnie 1 m), nie na sztywno. Gdy profil ma mapę kresek: najbliższa kreska i reszta poprawki. |
| `fit_control_shots` | Strzały kontrolne. Dla vacuum dopasowuje efektywną prędkość metodą najmniejszych kwadratów; grawitacja bez zmian, ≥2 strzały w tolerancji → możliwa lokalna kalibracja beta. Dla oporu/napędu — tylko log. |
| `air_to_ground_impact` / `air_to_ground_solve` | Rakieta ze śmigłowca: dystans poziomy, czas, kąt depresji punktu trafienia poniżej osi wyrzutni. W odwrotnym kierunku: kąt nurkowania dla zadanego dystansu (bisekcja w −30°…89°). |

**Założenia śmigłowca.** Rakieta startuje wzdłuż wektora prędkości lotu (bez kąta natarcia i ślizgu). Prędkość lotu dodaje się do prędkości rakiety — dziedziczenie jest założeniem do weryfikacji. Płaski teren, bez wiatru; napęd, jeśli jest, działa wzdłuż osi wyrzutni. Dla UB-32 / S-5K1 używana jest prędkość 650 m gry/s z katalogu (po potwierdzeniu danych społeczności).

**Tryb szybki (czołg / broń ręczna).** Opcja w zakładce „Broń bezpośrednia / śmigłowiec”:
- Numpad + / − przechodzi po presetach 200…2000 m (konfigurowalnych), Numpad 0 wraca do „bliski zasięg”.
- Skróty globalne są rejestrowane komendą `quick_range_hotkeys` wyłącznie przy włączonym trybie; UI pokazuje, czy rejestracja się udała. Po restarcie tryb jest wyłączony.
- Profil pochodzi z jednoznacznego odczytu panelu broni gry albo z ostatnio wybranego profilu.
- Nakładka pokazuje jedną linię, np. „800 m · PODNIEŚ 4,2 m (≈2,6 mrad) · t 0,9 s”, „< 480 m · CELUJ W ŚRODEK”, „300 m → kreska 3 (+0,4 m)” albo „brak danych pocisku — odczytaj z Loadout”. Jednostka „mrad” oznacza miliradiany, nie mile NATO 6400.

Testy: wartości vacuum zgadzają się z wzorami zamkniętymi (opad, czas lotu, elewacja `½·asin(g·r/v²)`, trafienie z nurkowania z dziedziczoną prędkością), zerowanie przecina linię celowania, opór zwiększa opad, problem odwrotny odtwarza dystans, a strzały kontrolne odtwarzają prędkość.
