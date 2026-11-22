# Cómo contribuir a CoupCat

¡Gracias por tu interés en CoupCat! Este documento explica, paso a paso, cómo
aportar. Está pensado para que lo siga **cualquier persona o cualquier modelo de IA**:
si descargas solo este archivo, tienes todo lo necesario para contribuir.

Repositorio: <https://github.com/MorionTutoriales/CoupCat>
Licencia: **AGPL-3.0-or-later** (ver `LICENSE`).

---

## Resumen del flujo (TL;DR)

1. **Busca o crea un issue** que describa el trabajo.
2. **Comenta el issue para tomarlo** (`/tomar`) y espera a que te lo asignen o a que
   nadie más lo esté haciendo.
3. **Sincroniza tu `main` con el del repositorio principal** (`upstream`) y **crea la
   rama desde ahí**. Es obligatorio **cada vez que tomes un issue**.
4. **Haz los cambios**, compila y ejecuta las pruebas.
5. **Vuelve a traer `main`** (`git merge upstream/main`) y **abre un Pull Request** que
   enlace el issue (`Closes #123`).
6. **Atiende la revisión**; al aprobarse, se hace *merge* a `main`.

Una tarea = un issue = una rama = un PR. No trabajes sin un issue asignado.

---

## Antes de empezar

- Lee el [README.md](README.md) para entender el proyecto y cómo compilar.
- Necesitas [Git](https://git-scm.com/) y, recomendado, el
  [GitHub CLI (`gh`)](https://cli.github.com/). Autentícate una vez con `gh auth login`.
- Para construir el proyecto en Windows se usa la cadena **MSVC** desde una *Developer
  Shell* de Visual Studio (ver el README). Compila el proyecto **antes** de empezar
  para confirmar que tu entorno funciona.

---

## Paso 1 — Elige o crea un issue

Todo cambio arranca con un issue. Así evitamos trabajo duplicado y dejamos registro
de la decisión.

**Para tomar algo existente:**

```bash
gh issue list --repo MorionTutoriales/CoupCat
gh issue view <NÚMERO> --repo MorionTutoriales/CoupCat
```

**Para proponer algo nuevo** (bug, mejora o función), crea un issue describiendo:
qué pasa o qué falta, por qué, y cómo reproducirlo si es un bug.

```bash
gh issue create --repo MorionTutoriales/CoupCat --title "Título claro" --body "Descripción…"
```

Si tienes dudas sobre el alcance, pregunta en el issue **antes** de escribir código.

---

## Paso 2 — Toma el issue (coméntalo)

Para evitar que dos personas trabajen lo mismo, **se reclama un issue comentándolo**.
Escribe un comentario con la palabra clave **`/tomar`** (o "Tomo este issue"):

```bash
gh issue comment <NÚMERO> --repo MorionTutoriales/CoupCat --body "/tomar"
```

Reglas de cortesía:

- Revisa que **nadie más** lo haya tomado ya (mira los comentarios y si está asignado).
- Un responsable te asignará el issue, o tú mismo si eres colaborador. Ese issue
  asignado es tu "luz verde".
- Si no puedes continuar, **avísalo en el issue** para que otra persona lo retome.
- Si un issue tomado queda inactivo mucho tiempo, puede liberarse.

---

## Paso 3 — Prepara tu entorno

Haz un *fork* del repositorio y clónalo (con `gh` es un solo paso):

```bash
gh repo fork MorionTutoriales/CoupCat --clone
cd CoupCat
```

### Sincroniza con `main` del repositorio principal (obligatorio con cada issue)

**Cada vez que tomes un issue**, y *antes* de escribir una sola línea, trae a tu copia
el `main` más reciente del repositorio principal. Otros PRs se mergean mientras tanto;
si partes de un `main` viejo, tu trabajo llegará con conflictos o se construirá sobre
código que ya cambió.

En tu fork, `origin` es *tu copia* y `upstream` es el repositorio principal
(`gh repo fork --clone` ya lo configura; compruébalo con `git remote -v` y, si falta,
añádelo con `git remote add upstream https://github.com/MorionTutoriales/CoupCat.git`):

```bash
git switch main
git fetch upstream
git merge --ff-only upstream/main     # tu main queda igual que el del repositorio principal
git push origin main                  # opcional: actualiza también tu fork en GitHub
```

Después crea la rama **desde ese `main` actualizado**, con una descripción corta y el
número del issue:

```bash
git switch -c feat/123-descripcion-corta    # funciones nuevas
# o
git switch -c fix/123-descripcion-corta     # correcciones
```

Reglas:

- **Una rama nueva por issue.** No reutilices la rama de un issue anterior.
- Tu `main` local **no debe llevar cambios propios**: trabaja siempre en ramas. Si
  `git merge --ff-only` falla porque tu `main` tiene commits suyos, guárdalos primero
  (`git branch respaldo`) y luego iguálalo con `git reset --hard upstream/main`.
- Si **retomas** una rama que ya tenías, no basta con crearla de nuevo: tráele los
  cambios con `git merge upstream/main` (ver Paso 6) antes de seguir.

---

## Paso 4 — Haz los cambios

- **Mantén el PR enfocado** en el issue: un solo tema por PR.
- **Comprobaciones (ligeras a propósito):** el proyecto está empezando, así que no
  pedimos una batería pesada. Compilar este proyecto tarda (FFmpeg, wgpu, ONNX Runtime
  y whisper.cpp), por eso **limita siempre los comandos al crate que tocaste** y no
  uses `--all` ni ejecutes todo el workspace. Dentro de `src/`:

  ```bash
  # Obligatorio: confirma que compila (cambia el crate si no tocaste la interfaz)
  cargo check -p coupcat-desktop
  ```

  - **Recomendado:** `cargo clippy -p coupcat-desktop`, si no te supone esperar mucho.
  - **Solo si tocaste el motor** (`src/crates/concat-*`): `cargo test -p <ese-crate>`.
    Los cambios de interfaz no necesitan ejecutar pruebas.
  - **Formato:** pasa `rustfmt` **solo a los archivos que modificaste**
    (`rustfmt --edition 2024 ruta/al/archivo.rs`). No uses `cargo fmt --all`: reformatea
    archivos que no tocaste y llena el PR de ruido.
  - Si una comprobación tarda demasiado en tu equipo, **puedes omitirla**: dilo en el
    PR (por ejemplo, "solo `cargo check`") y la revisión lo tendrá en cuenta.
- **Commits claros:** mensajes breves en imperativo que expliquen el *qué* y el *por
  qué*. En inglés o español, pero sé consistente dentro del PR.
- **Licencia:** al contribuir aceptas que tu aporte se publique bajo **AGPL-3.0-or-later**.
  No incluyas código, recursos (imágenes, fuentes, modelos) ni dependencias cuya
  licencia sea incompatible. Si agregas una dependencia o recurso de terceros,
  menciónalo en el PR y actualiza `THIRD_PARTY_NOTICES.md` si aplica.

---

## Paso 5 — Abre el Pull Request

**Antes de abrir el PR, vuelve a traer `main`.** Mientras trabajabas pudieron mergearse
otros cambios; actualiza tu rama, resuelve los conflictos si los hay y **compila otra
vez** para comprobar que todo sigue funcionando junto:

```bash
git fetch upstream
git merge upstream/main
```

Después sube tu rama y abre el PR hacia `main`, **enlazando el issue** para que se
cierre solo al hacer *merge*:

```bash
git push -u origin feat/123-descripcion-corta
gh pr create --repo MorionTutoriales/CoupCat --base main \
  --title "Resumen del cambio" \
  --body "Closes #123

Qué cambia y por qué. Cómo se probó."
```

En la descripción del PR incluye:

- `Closes #123` (o `Fixes #123`) para vincular el issue.
- Qué cambiaste y por qué.
- Cómo lo probaste (comandos, pasos, capturas si es de interfaz).

---

## Paso 6 — Revisión y *merge*

- Un responsable revisará tu PR y podrá pedir cambios. Responde a los comentarios y
  sube nuevos commits a la misma rama; el PR se actualiza solo.
- Si `main` avanzó mientras tu PR estaba en revisión (otro PR se mergeó antes que el
  tuyo), **trae esos cambios a tu rama** del mismo modo que antes de abrirlo
  (`upstream` es el repositorio principal; ver el Paso 3):

  ```bash
  git fetch upstream
  git merge upstream/main
  git push origin feat/123-descripcion-corta
  ```

  Con `merge` no hace falta forzar el *push*. Si prefieres un historial lineal puedes
  usar `git rebase upstream/main`, pero entonces tendrás que subir con
  `git push --force-with-lease origin feat/123-descripcion-corta`.

  Si hay conflictos, resuélvelos (en un `merge`, `git add` y `git commit`; en un
  `rebase`, `git rebase --continue`) y vuelve a subir. Si no puedes, avísalo en el PR y
  una persona responsable puede actualizarla por ti.
- Cuando el PR esté aprobado y la CI (si existe) en verde, se hará *merge* y el issue
  se cerrará automáticamente.

---

## Notas para modelos de IA / agentes

Si eres un agente trabajando de forma autónoma, sigue exactamente estas reglas:

1. **No escribas código sin un issue asignado.** Primero toma el issue con `/tomar`
   (Paso 2) y confirma que está asignado a ti.
2. **Cada vez que tomes un issue, sincroniza primero con `main`.** Antes de escribir
   código: `git switch main`, `git fetch upstream`, `git merge --ff-only upstream/main`,
   y crea la rama **desde ese `main` actualizado** (Paso 3). Nunca partas de una rama o
   de un `main` que no hayas actualizado en ese momento.
3. **Un issue → una rama → un PR.** No mezcles temas no relacionados ni reutilices la
   rama de un issue anterior.
4. **Antes de abrir el PR**, trae otra vez `main` (`git fetch upstream` y
   `git merge upstream/main`), resuelve los conflictos si los hay y ejecuta al menos
   `cargo check -p <crate-tocado>` dentro de `src/` (ver Paso 4; evita `--all` y los
   comandos sobre todo el workspace). Reporta en el PR **exactamente qué ejecutaste y qué
   resultado dio**. No afirmes que algo pasó si no lo ejecutaste.
5. **No hagas push a `main`.** Trabaja siempre en tu fork/rama. Para actualizar tu rama
   usa `merge` (no requiere forzar). Si usas *rebase*, solo se permite
   `--force-with-lease` sobre **tu propia rama del PR**, nunca sobre `main` ni sobre
   ramas ajenas.
6. **No toques** `LICENSE`, `NOTICE` ni el historial de atribución sin indicarlo
   explícitamente en el issue.
7. Enlaza el issue con `Closes #<n>` en el PR y describe cómo verificaste el cambio.

---

## Código de conducta

Sé respetuoso y constructivo. Este es un espacio para construir algo útil entre
personas (y agentes) de todos los niveles. Comentarios y revisiones se hacen sobre el
código, no sobre quien lo escribe.

¿Dudas? Abre un issue con la etiqueta de pregunta o coméntalo en el issue relacionado.
¡Gracias por aportar a CoupCat!
