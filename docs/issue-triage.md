# Clasificador de issues de CoupCat con Gemini

Implementación del issue #1. La automatización permanece desactivada hasta configurar TRIAGE_ENABLED=true y el secreto GEMINI_API_KEY.
Solo usa Python 3 y su biblioteca estándar. No cambia el código Rust de CoupCat.

## 1. Incorporar los archivos

Copiar `.github/workflows/issue-triage.yml`, `.github/triage-config.json`,
`scripts/issue_triage.py` y `scripts/test_issue_triage.py` a las mismas rutas
dentro de CoupCat. Revisarlos en una rama y llevarlos a `main` mediante un PR.
No reemplazar otras carpetas ni el workflow de releases existente.
Los eventos de issues necesitan que el workflow exista en la rama predeterminada.

## 2. Crear la clave

En https://aistudio.google.com/apikey crear una clave para un proyecto de Google.
Revisar la disponibilidad, cuotas y precio del modelo elegido. No activar
facturación sin decidir primero el presupuesto. Este paquete no hace llamadas
de pago durante sus pruebas locales. El uso real sí consume cuota de Gemini.

En CoupCat: Settings > Secrets and variables > Actions > Secrets >
New repository secret. Nombre: `GEMINI_API_KEY`. Valor: la clave de AI Studio.
No ponerla en el repositorio, en issues, capturas ni mensajes.
`GITHUB_TOKEN` lo proporciona Actions automáticamente; no crear un PAT.

## 3. Configurar variables

En la pestaña Variables de la misma pantalla:

- `GEMINI_MODEL`: identificador exacto de un modelo disponible en tu cuenta que
  admita Interactions API y salida JSON estructurada. El valor predeterminado del
  paquete es `gemini-4-hydrogen`, usado en la documentación oficial consultada.
- `TRIAGE_ENABLED`: dejar en `false` mientras se prepara. Cambiar a `true` cuando
  quieras habilitar ejecuciones, incluidas las manuales. A partir de entonces los
  issues nuevos, reabiertos o editados pueden consumir cuota y publicar resultados.

La API recibe el título y hasta 10000 caracteres del cuerpo del issue, junto con
hasta 20 candidatos (título y hasta 2500 caracteres de cuerpo por candidato).
No se envía código del proyecto ni adjuntos. `store: false` desactiva el historial
de Interactions; no sustituye las condiciones de datos del proveedor.

## 4. Probar con un issue real

Con el workflow ya en `main` y TRIAGE_ENABLED=true: Actions >
Clasificar issues con Gemini > Run workflow. Elegir `main`, indicar el número de
un issue abierto y dejar `dry_run` activado. Consume una llamada a Gemini pero no
comenta ni etiqueta. Ver la propuesta en el resumen de esa ejecución.

Si es correcta, repetir desactivando `dry_run`. El bot creará, si hace falta,
la etiqueta y publicará un comentario. Editar el texto de ese issue comprobará
que actualiza su propio comentario en vez de duplicarlo.
Si la API responde 400/404, verificar el modelo y su compatibilidad; 401/403,
clave y permisos; 429, cuota. No se reintenta indefinidamente ni se imprimen claves.

## Comportamiento

- Prioridades: critical/high/medium/low; rúbrica editable en triage-config.json.
- Con confianza estimada inferior a 0.75: `triage: needs-review`, sin prioridad nueva.
- Una prioridad existente nunca se reemplaza. Para reclasificar, quitar su etiqueta
  `priority:` y ejecutar manualmente. Las etiquetas ajenas no se borran.
- La etiqueta `triage:manual` excluye el issue del análisis y la publicación.
- Los relacionados son sugerencias, nunca cierres automáticos ni duplicados confirmados.
- Busca entre las 300 entradas más recientemente actualizadas (el endpoint incluye
  PR, que se descartan) y preselecciona hasta 20 issues por similitud léxica. Es una
  búsqueda acotada, no exhaustiva ni multilingüe semántica. No tener candidatos en
  un repositorio nuevo es normal.
- Un máximo de una llamada a Gemini por ejecución; un timeout de 5 minutos por job.
  No hay límite mensual de gasto incorporado: configurar cuota en Google y desactivar
  TRIAGE_ENABLED si hace falta. No se ejecutan instrucciones contenidas en issues.
- Si se cambia el issue durante el análisis, se descarta el resultado obsoleto.
- El script valida las referencias devueltas antes de publicar y solo modifica su
  propio comentario. Las operaciones de etiqueta y comentario no son una transacción;
  ante un fallo parcial, una ejecución posterior puede completar el comentario.

## Nombre y avatar propios (segunda fase)

No basta con cambiar el nombre del workflow: seguirá publicando github-actions[bot].
Registrar una GitHub App propia en Settings de la cuenta > Developer settings >
GitHub Apps. Elegir nombre e imagen de perfil; para usarla desde Actions no hace
falta alojar un servidor de webhooks. Darle permiso Issues: read and write e
instalarla solo en CoupCat. Guardar su App ID y clave privada mediante variables
y secretos. Generar un installation token con la acción oficial
actions/create-github-app-token (fijada a una revisión verificada).

Pasar ese token al script como GH_TOKEN y su login real de bot como
TRIAGE_BOT_LOGIN. Mantener el token de lectura de Actions para checkout. La lógica
de clasificación no cambia. Esta fase requiere configurar y autorizar la App;
el paquete no la crea ni concede permisos. Al cambiar de identidad, conservar o
retirar manualmente el comentario anterior de Actions para no tener dos visibles.

## Validación local

Desde la raíz del paquete o del repositorio:

```shell
python -m unittest discover -s scripts -p test_issue_triage.py
```

Estas pruebas usan respuestas simuladas. No acreditan que la clave tenga cuota
ni que Gemini o GitHub acepten la integración real: eso lo comprueba el paso 4.

## Documentación

- https://aistudio.google.com/docs/structured-output
- https://aistudio.google.com/docs/interactions-overview
- https://aistudio.google.com/apikey
- https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets
- https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app
