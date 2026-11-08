# NPC Spawner 1.0.0

Invoca a cualquier enemigo, jefe o NPC de Elden Ring donde estés, para probar builds, ajustes y mods.

Hecho y probado en **Elden Ring 1.17.2 (eldenring.exe 2.7.2.0), versión mundial, PC**. Las demás versiones no están probadas.
**Solo sin conexión**, con Easy Anti-Cheat desactivado. Usa el creador de personajes de depuración del propio juego.

## Qué hace

- **Invocar a cualquiera:** todas las filas de NpcParam del juego y del DLC, por nombre (en el idioma del juego o en inglés) o por ID.
- **Listas ordenadas:** por tipo (jefes, enemigos, invasores y NPC hostiles, cenizas y aliados, otros NPC) y por la zona de los mapas del juego donde aparece cada uno (114 zonas en 9 áreas).
- **Uno por personaje** por defecto: una sola fila por personaje, sin filas sin nombre, muñecos ni personajes que no combaten. «Every row» las muestra todas, con las variantes de cada zona.
- **Su propia IA:** cada personaje aparece con la IA que le dan los mapas del juego.
- **Cuántos, dónde y cómo:** hasta 20 de una vez, delante de ti, encima, sobre tu objetivo fijado, en un punto guardado o al azar; en línea, en círculo o en cuadrícula.
- **Bando:** hostil, de tu lado o neutral. Al cambiarlo cambian también los ya invocados.
- **Los aliados no se quedan atrás:** un aliado que se queda a más de 20 m durante 4 segundos pasa a estar detrás de ti (`FollowDistance`, 0 = desactivado). No te siguen andando por su cuenta.
- **IA:** activa, en pausa o quieta (se anima, pero ni se mueve ni ataca).
- **Vida:** vida al aparecer, vida infinita, invulnerable.
- **Reiniciar a todos** (curarlos y devolverlos a su sitio; los muertos vuelven a aparecer), **quitar el último**, **quitar a todos**.
- **Límite de personajes activos:** el juego mantiene unos 60 personajes activos y desactiva enemigos del mapa por cada invocado. Subir el límite los mantiene (experimental, cuesta rendimiento).

## Dos menús

- **F11:** un menú de diálogo, como el de un mercader, con todas las acciones y ajustes y las listas por tipo y por zona. No necesita ningún otro mod. Con NativeTweaks r157 o posterior (necesita el ERNativeUI que viene con él), este menú es la fila **NPC Spawner** del menú de diálogo común de NativeTweaks (F11 por defecto) y la tecla F11 propia de NPC Spawner no hace nada.
- **NativeTweaks:** con [NativeTweaks](https://www.nexusmods.com/eldenring/mods/11339) r155 o posterior, el menú completo está en el menú Sistema, pestaña NativeTweaks, entrada NPC Spawner, con el buscador por nombre y sus filtros. Usa el ERNativeUI que trae NativeTweaks; con el ERNativeUI oficial los resultados de la búsqueda no se actualizan.

## Atajos

Los eliges tú: cualquier tecla con Ctrl, Shift o Alt, o ninguna, para cada acción. Se asignan en el menú de diálogo (Spawn options > Hotkeys: elige una acción y pulsa las teclas nuevas), en la pestaña de NativeTweaks (Hotkeys) o en `[Hotkeys]` de `NpcSpawner.ini`. Los cambios se aplican al momento, sin reiniciar. Solo funcionan con la ventana del juego delante. Los de serie:

| Teclas | Acción |
|---|---|
| F11 | Menú de diálogo (con NativeTweaks r157+: su menú F11, fila NPC Spawner) |
| Ctrl+F6 | Invocar con los ajustes actuales |
| Ctrl+Shift+F6 | Invocar otra vez el último |
| Ctrl+F5 | Guardar tu posición como punto de aparición |
| Ctrl+F7 | IA de todos: activa, en pausa, quieta |
| Ctrl+F8 | Bando de todos: el suyo o el tuyo |
| Ctrl+F9 | Reiniciar a todos |
| Ctrl+F10 | Quitar el último |
| Ctrl+Shift+F10 | Quitar a todos |

## Instalación

El archivo trae una carpeta `DLLs` con `NpcSpawner.dll` y `NpcSpawner.ini`. Mantenlos juntos.

- **Mod Organizer 2:** instala el archivo como un mod y actívalo. Tu MO2 necesita un soporte de Elden Ring que cargue los mods de DLL desde la carpeta DLLs de cada mod.
- **Mod Engine 3:** añade `DLLs/NpcSpawner.dll` a `[[natives]]` en tu perfil.
- **Mod Engine 2:** copia la carpeta DLLs dentro de `mod` y añade `mod\DLLs\NpcSpawner.dll` a `external_dlls` en `config_eldenring.toml`.

Con NativeTweaks, carga esta DLL **después de ERNativeUI.dll y antes de NativeTweaks.dll**: si carga demasiado tarde, falta la entrada del menú (el log dice «register_provider failed (8)») y el menú de F11 sigue funcionando.

## Ajustes y log

Todas las opciones están en `NpcSpawner.ini`, y cada línea está explicada. Se vuelve a leer en cada invocación. «Save these settings» en el menú lo escribe.
En cuanto un menú cambia un ajuste, se usan los ajustes del menú en vez de los del INI hasta cerrar el juego (los atajos siguen al INI cuando cambia).
El límite de personajes activos y los presupuestos de actualización (`MaxActive`, `UpdateNear`, `UpdateFar`) siguen subidos hasta cerrar el juego, aunque se quiten todos los invocados.
El log es `NpcSpawner.log`, junto a la DLL (con MO2, en `overwrite\DLLs`). Empieza de cero en cada arranque.

## Conviene saber

- Los invocados no se guardan: desaparecen en la siguiente pantalla de carga.
- Los personajes que solo salen en cinemáticas o que forman parte de un jefe mayor pueden quedarse quietos o verse mal, y algunos jefes dependen de los eventos de su arena.
- Hasta 76 invocados a la vez (la lista de depuración del juego), 30 por defecto.

## Créditos

- Nombres de las filas de NpcParam y de las zonas de los mapas: [Smithbox](https://github.com/vawser/Smithbox) de Vawser (MIT).
- Bindings de Elden Ring fromsoftware-rs de Vincent Swarte y colaboradores, pelite y windows-rs de Microsoft.

Consulta `THIRD-PARTY-NOTICES.txt`.
