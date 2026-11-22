# Seguridad

## Versiones soportadas

Solo la última versión de la rama `main` y la última release publicada reciben
correcciones de seguridad.

| Versión | Soporte |
|---|---|
| `0.1.x` | Sí |
| Anteriores | No |

## Cómo reportar una vulnerabilidad

**No abras un issue público para problemas de seguridad.**

Usa el reporte privado de GitHub:

<https://github.com/fiopans1/F-Terminal/security/advisories/new>

Si esa página no está disponible, contacta con [@fiopans1](https://github.com/fiopans1)
por GitHub para acordar un canal privado.

Incluye, si es posible:

- Versión o commit afectado y plataforma.
- Pasos mínimos para reproducirlo.
- Impacto esperado (ejecución de comandos, fuga de datos, escape del PTY,
  sobrescritura de archivos, etc.).
- Logs o capturas con tokens y rutas privadas eliminados.
- Cualquier mitigación que hayas identificado.

## Qué entra en el alcance

- Manejo de secuencias VT/OSC y del estado del terminal.
- Aislamiento entre el PTY, el proceso hijo y la UI.
- Portapapeles, bracketed paste y respuestas automáticas del terminal.
- Almacenamiento y uso de tokens de GitHub/GitLab.
- Ejecución de `git` y `curl` desde la aplicación.
- Instaladores, scripts de empaquetado y workflows de release.
- Dependencias con vulnerabilidades conocidas.

## Fuera del alcance

- Bugs que requieran acceso físico o una cuenta ya comprometida.
- Ataques de denegación de servicio locales triviales.
- Vulnerabilidades en el shell, Git, curl o el sistema operativo anfitrión.
- Falta de firma/notarización si el instalador se descarga de una fuente que no
  sea la release oficial.

## Qué hacemos

- Acusaremos recibo en un plazo orientativo de 7 días.
- Investigaremos y mantendremos informado al reportero.
- Publicaremos un aviso coordinado cuando exista una corrección.
- Acreditaremos al reportero si lo desea.

No hay programa de bug bounty; las contribuciones de seguridad se reconocen en
el aviso o en el CHANGELOG.

## Buenas prácticas para usuarios

- Descarga instaladores solo desde las releases oficiales del repositorio.
- Los archivos de tokens se crean con permisos `600` en Unix; no los compartas.
- Revisa los comandos y repositorios antes de ejecutar operaciones Git
  destructivas.
- F-Terminal no envía telemetría. Las únicas peticiones de red de la aplicación
  son a las APIs de GitHub/GitLab configuradas.
