# Objetivo

Analizar y rediseñar la vista SONG existente de SeqTerm para transformarla en un entorno de producción musical profesional basado en línea de tiempo (timeline), inspirado conceptualmente en DAWs como Pro Tools, Reaper, Cubase, Logic Pro y Ableton Arrangement View.

La vista SONG debe convertirse en el centro de composición, arreglo, edición temporal y organización del proyecto musical.

Importante:

* NO integrar funciones de mezcla (Mixer).
* NO duplicar funcionalidades de la vista Mixer existente.
* Mantener una separación clara entre composición/arreglo y mezcla.
* Adaptar el diseño al paradigma visual y técnico de SeqTerm.
* No copiar interfaces de otros productos; utilizar únicamente sus conceptos operativos.

---

# Visión General

La vista SONG debe evolucionar desde un secuenciador básico hacia un verdadero Arrangement Editor profesional.

Debe permitir:

* Organización completa del proyecto.
* Edición temporal precisa.
* Manipulación de clips.
* Automatizaciones.
* Navegación eficiente.
* Edición no destructiva.
* Flujo de trabajo orientado a producciones complejas.

La experiencia debe sentirse cercana a:

* Pro Tools Edit Window
* Reaper Arrange View
* Cubase Project Window
* Logic Tracks Area

---

# Fase 1: Auditoría de la implementación actual

Analizar:

* Arquitectura de la vista SONG actual.
* Modelo de tracks.
* Modelo de clips.
* Sistema de reproducción.
* Sincronización temporal.
* Sistema de eventos.
* Sistema de rendering.
* Gestión de selección.
* Gestión de zoom.

Generar un informe identificando:

* Limitaciones actuales.
* Problemas de UX.
* Cuellos de botella.
* Riesgos de escalabilidad.
* Componentes reutilizables.
* Componentes que requieren refactorización completa.

---

# Fase 2: Nuevo modelo conceptual

La vista SONG debe representar una línea de tiempo global del proyecto.

Jerarquía:

Project
↓
Tracks
↓
Lanes
↓
Clips
↓
Events

Cada elemento debe ser editable independientemente.

---

# Fase 3: Layout profesional

Implementar una disposición similar a los DAWs modernos.

## Header Global

Controles de transporte:

* Play
* Stop
* Record
* Loop
* Metronome
* Tempo
* Time Signature

Indicadores:

* Tiempo actual
* Compás actual
* BPM
* Estado de grabación

---

## Track Inspector

Panel lateral izquierdo.

Cada track debe mostrar:

* Nombre
* Color
* Tipo
* Estado de armado
* Solo
* Mute
* Monitor

Opcional:

* Ícono de instrumento
* Ícono de audio

No incluir controles de mezcla.

---

## Timeline Principal

Zona principal del editor.

Debe contener:

* Regla temporal
* Marcadores
* Tracks
* Clips
* Automatizaciones

El timeline debe ser el foco visual principal.

---

# Fase 4: Sistema de Clips

Implementar clips como entidades independientes.

Tipos:

## Audio Clips

Representación visual:

* Waveform
* Nombre
* Color

Funciones:

* Move
* Trim
* Split
* Duplicate
* Loop
* Stretch

---

## MIDI Clips

Representación visual:

* Nombre
* Color
* Indicadores de contenido

Funciones:

* Move
* Resize
* Duplicate
* Loop

---

## Pattern Clips

Para secuencias provenientes del secuenciador interno.

Funciones:

* Reutilización.
* Referencias compartidas.
* Instanciación múltiple.

---

# Fase 5: Edición profesional

Implementar herramientas de edición estándar.

## Selection Tool

* Selección simple.
* Selección múltiple.
* Selección rectangular.

---

## Split Tool

Dividir clips en posición actual.

Shortcut:

S

---

## Trim Tool

Modificar duración sin alterar contenido original.

---

## Move Tool

Mover clips.

Snap configurable.

---

## Duplicate Tool

Duplicación rápida.

Alt + Drag

---

## Time Stretch Tool

Modificar duración temporal.

Manteniendo pitch cuando sea posible.

---

# Fase 6: Sistema de Automatización

Agregar automatizaciones directamente en tracks.

Parámetros automatizables:

* Volumen de pista
* Panorama
* Parámetros de instrumentos
* Parámetros de efectos
* Parámetros granulares
* Parámetros de sampler

Cada automatización debe soportar:

* Puntos
* Curvas
* Rampas
* Bézier

Visualización integrada en el timeline.

---

# Fase 7: Navegación avanzada

Implementar navegación de nivel profesional.

## Zoom

Horizontal:

* Wheel + Ctrl
* Gestos equivalentes

Vertical:

* Wheel + Alt

---

## Scroll

* Horizontal
* Vertical
* Inercial opcional

---

## Zoom to Selection

Shortcut:

Z

---

## Fit Project

Shortcut:

Shift+F

---

## Fit Track

Shortcut:

F

---

# Fase 8: Sistema de Marcadores

Agregar soporte completo para:

## Markers

* Intro
* Verse
* Chorus
* Bridge
* Outro

---

## Regions

* Inicio
* Fin
* Color
* Nombre

---

## Cycle Regions

Para reproducción repetitiva.

---

# Fase 9: Gestión de Tracks

Soportar:

## Audio Tracks

Clips de audio.

---

## Instrument Tracks

Eventos MIDI.

---

## Hybrid Tracks

Audio + MIDI.

---

## Folder Tracks

Agrupación visual.

---

## Group Tracks

Organización lógica.

---

# Fase 10: Arranger avanzado

Agregar herramientas para producción musical moderna.

## Sections

Definir:

* Intro
* Verse
* Chorus
* Bridge
* Outro

Como bloques visuales.

---

## Rearrangement

Permitir:

* Mover secciones completas.
* Duplicar secciones.
* Reordenar estructura.

---

## Arrangement Overview

Mini mapa global del proyecto.

Permitir navegación rápida.

---

# Fase 11: Mejoras críticas de UX

Resolver problemas actuales de interacción.

## Mouse

Implementar comportamiento consistente.

Click:

* Selección.

Double Click:

* Abrir editor asociado.

Triple Click:

* Seleccionar clip completo.

Drag:

* Mover clips.
* Selección múltiple.

Alt + Drag:

* Duplicar.

Shift + Drag:

* Restricción de movimiento.

---

## Teclado

### Transporte

Space = Play/Stop

Enter = Return to Start

R = Record

---

### Navegación

Home = Inicio proyecto

End = Final proyecto

PageUp = Zoom In

PageDown = Zoom Out

---

### Edición

Ctrl+C = Copy

Ctrl+V = Paste

Ctrl+X = Cut

Delete = Delete

Ctrl+D = Duplicate

S = Split

Z = Zoom Selection

---

# Fase 12: Escalabilidad

La arquitectura debe soportar:

* Cientos de tracks.
* Miles de clips.
* Proyectos extensos.
* Automatizaciones complejas.

Optimizar:

* Rendering.
* Virtualización.
* Cache visual.
* Redibujado incremental.

Evitar:

* Re-render completo del timeline.
* Recalcular waveforms innecesariamente.
* Operaciones costosas en cada frame.

---

# Fase 13: Integración con el resto de SeqTerm

La vista SONG debe actuar como centro del proyecto.

Integraciones:

Editor View
↓
Pattern View
↓
Song View
↓
Mixer View
↓
Master Output

La vista SONG debe consumir contenido generado por:

* Sampler.
* Editor de audio.
* Síntesis granular.
* Secuenciador.
* Instrumentos.

Sin asumir responsabilidades de mezcla.

---

# Resultado esperado

Transformar la vista SONG en un verdadero Arrangement Editor profesional comparable conceptualmente a los DAWs modernos.

Debe convertirse en el espacio principal para:

* Composición.
* Arreglo.
* Edición temporal.
* Organización del proyecto.
* Automatización.

Manteniendo el Mixer como una vista completamente independiente.

Entregar:

1. Diagnóstico de la implementación actual.
2. Arquitectura propuesta.
3. Modelo de datos actualizado.
4. Lista de cambios por archivo.
5. Plan de migración incremental.
6. Implementación completa.
7. Documentación técnica.
8. Manual de usuario.
