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
3. **Crea una rama** desde `main` en tu fork.
4. **Haz los cambios**, compila y ejecuta las pruebas.
5. **Abre un Pull Request** que enlace el issue (`Closes #123`).
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

Crea una rama descriptiva desde `main`, incluyendo el número del issue:

```bash
git switch -c feat/123-descripcion-corta    # funciones nuevas
# o
git switch -c fix/123-descripcion-corta     # correcciones
```

---

## Paso 4 — Haz los cambios

- **Mantén el PR enfocado** en el issue: un solo tema por PR.
- **Estilo de código:** antes de hacer commit, dentro de `src/` ejecuta:

  ```bash
  cargo fmt --all
  cargo clippy --all-targets
  cargo test
  cargo build -p coupcat-desktop
  ```

  El código debe compilar sin errores y pasar `clippy` y las pruebas. Si añades
  comportamiento nuevo, añade o actualiza pruebas cuando tenga sentido.
- **Commits claros:** mensajes breves en imperativo que expliquen el *qué* y el *por
  qué*. En inglés o español, pero sé consistente dentro del PR.
- **Licencia:** al contribuir aceptas que tu aporte se publique bajo **AGPL-3.0-or-later**.
  No incluyas código, recursos (imágenes, fuentes, modelos) ni dependencias cuya
  licencia sea incompatible. Si agregas una dependencia o recurso de terceros,
  menciónalo en el PR y actualiza `THIRD_PARTY_NOTICES.md` si aplica.

---

## Paso 5 — Abre el Pull Request

Sube tu rama y abre el PR hacia `main`, **enlazando el issue** para que se cierre solo
al hacer *merge*:

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
- Mantén tu rama al día con `main` si te lo piden (`git pull --rebase origin main`).
- Cuando el PR esté aprobado y la CI (si existe) en verde, se hará *merge* y el issue
  se cerrará automáticamente.

---

## Notas para modelos de IA / agentes

Si eres un agente trabajando de forma autónoma, sigue exactamente estas reglas:

1. **No escribas código sin un issue asignado.** Primero toma el issue con `/tomar`
   (Paso 2) y confirma que está asignado a ti.
2. **Un issue → una rama → un PR.** No mezcles temas no relacionados.
3. **Antes de abrir el PR**, ejecuta `cargo fmt --all`, `cargo clippy --all-targets`,
   `cargo test` y `cargo build -p coupcat-desktop` dentro de `src/`, y reporta el
   resultado real en el PR. No afirmes que algo pasó si no lo ejecutaste.
4. **No hagas push a `main`** ni fuerces el historial. Trabaja siempre en tu fork/rama.
5. **No toques** `LICENSE`, `NOTICE` ni el historial de atribución sin indicarlo
   explícitamente en el issue.
6. Enlaza el issue con `Closes #<n>` en el PR y describe cómo verificaste el cambio.

---

## Código de conducta

Sé respetuoso y constructivo. Este es un espacio para construir algo útil entre
personas (y agentes) de todos los niveles. Comentarios y revisiones se hacen sobre el
código, no sobre quien lo escribe.

¿Dudas? Abre un issue con la etiqueta de pregunta o coméntalo en el issue relacionado.
¡Gracias por aportar a CoupCat!
