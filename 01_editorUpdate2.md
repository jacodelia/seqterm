OBJETIVO

Rediseñar y expandir el subsistema de instrumentos, plugins y edición sonora de SeqTerm para convertirlo en una plataforma extensible comparable a un DAW moderno, con una arquitectura unificada para instrumentos, efectos, automatización, modulación y gestión de presets.

Repositorio objetivo:

https://github.com/jacodelia/seqterm

PRINCIPIO DE DISEÑO

NO implementar lógica específica de SF2, LV2, VST o futuros formatos directamente en la interfaz de usuario.

Diseñar una arquitectura basada en abstracciones comunes.

La UI debe interactuar únicamente con una API genérica de parámetros y modulación.

Los formatos concretos deben implementarse mediante adaptadores.

FORMATOS SOPORTADOS

Implementar soporte actual y futuro para:

* SF2 (SoundFont 2)
* SFZ
* LV2
* VST2
* VST3
* CLAP (arquitectura preparada desde el inicio)
* Instrumentos internos de SeqTerm

ARQUITECTURA CENTRAL

Crear una capa denominada Universal Instrument Engine.

Interfaces principales:

IPluginHost
IPluginInstance
IParameterProvider
IParameterEditor
IPresetManager
IModulationMatrix
IAutomationEngine
ISampleBasedInstrument
IInstrumentAdapter

Adaptadores:

SF2Adapter
SFZAdapter
LV2Adapter
VSTAdapter
CLAPAdapter

Todos deben exponer una API homogénea.

MODELO UNIVERSAL DE PARÁMETROS

Implementar:

```cpp
struct Parameter
{
    std::string id;
    std::string name;

    ParameterType type;

    double value;
    double minimum;
    double maximum;
    double defaultValue;

    std::string unit;

    bool automatable;
    bool modulatable;
    bool readOnly;

    std::vector<std::string> enumValues;
};
```

ParameterType:

* Float
* Integer
* Boolean
* Enum
* Trigger
* String

Los parámetros de SF2, LV2, VST y CLAP deben convertirse internamente a este modelo.

MODULATION MATRIX UNIVERSAL

Implementar una matriz de modulación independiente del formato.

Fuentes:

* MIDI CC
* Velocity
* Aftertouch
* Poly Aftertouch
* Pitch Bend
* LFO interno
* Envelope interno
* Step Sequencer
* Macro Controls
* MIDI Learn
* Audio Envelope Follower

Destinos:

* Cualquier parámetro automatable
* Cualquier parámetro modulatable

Estructura:

ModulationRoute
{
source;
destination;
amount;
curve;
polarity;
enabled;
}

Características:

* Múltiples rutas simultáneas
* Modulación bipolar/unipolar
* Curvas personalizadas
* Escalado
* Transformaciones

AUTOMATION ENGINE

Implementar automatización universal.

Modos:

* Read
* Write
* Touch
* Latch

Curvas:

* Linear
* Exponential
* Logarithmic
* Bezier

Características:

* Automatización por pista
* Automatización por clip
* Automatización por patrón
* Automatización en tiempo real
* Grabación de automatización desde GUI
* Grabación de automatización desde MIDI

SISTEMA DE MACROS

Agregar:

Macro 1–16

Cada macro puede controlar múltiples parámetros simultáneamente.

Ejemplo:

Macro 1:
Filter Cutoff (+80%)
Resonance (+20%)
Reverb Mix (-40%)

INTERFAZ DE EDICIÓN DE SF2

Implementar un editor completo de SoundFonts.

SECCIÓN DE MUESTRAS

* Importar WAV
* Reemplazar WAV
* Renombrar muestras
* Visualizador de forma de onda
* Normalización
* Ajuste de ganancia

SECCIÓN DE LOOPS

* Loop Start
* Loop End
* Loop Crossfade
* Forward Loop
* Ping Pong Loop
* Loop Preview

SECCIÓN DE ZONAS

* Key Range
* Velocity Range
* Root Key
* Fine Tune
* Coarse Tune

SECCIÓN DE ENVOLVENTES

* Attack
* Hold
* Decay
* Sustain
* Release

SECCIÓN DE FILTROS

* LPF
* HPF
* BPF

Parámetros:

* Cutoff
* Resonance
* Tracking

SECCIÓN LFO

* Vibrato
* Tremolo
* Filter LFO

Parámetros:

* Frequency
* Delay
* Depth
* Waveform

EDITOR DE MAPEO

Implementar teclado virtual interactivo.

Funciones:

* Drag & Drop
* Split automático
* Velocity Layers
* Round Robin preparado para SFZ

PLUGIN INSPECTOR

Implementar inspección dinámica.

Mostrar:

* Nombre
* Tipo
* Unidad
* Rango
* Valor actual
* Valor por defecto

Generar controles automáticamente:

Float → Slider o Knob
Boolean → Switch
Enum → Selector
Integer → SpinBox

HOST CLAP

Preparar arquitectura para CLAP desde el inicio.

Requisitos:

* Event-driven processing
* Thread-safe parameter updates
* Sample accurate automation
* Per-note automation
* Per-note modulation
* Voice information

No implementar hacks específicos de VST.

Diseñar primero para CLAP y adaptar VST/LV2 al modelo.

PRESETS

Implementar:

* Guardar
* Cargar
* Duplicar
* Snapshot A/B
* Comparar
* Exportar JSON
* Importar JSON

Formato:

Preset
{
metadata
parameters
modulation
automation
}

MIDI LEARN

Agregar:

* Learn automático
* Mapeo manual
* Curvas de respuesta
* Persistencia

PERFORMANCE

Garantizar:

* Cambios de parámetros sin cortes de audio
* Lock-free cuando sea posible
* Actualización en tiempo real
* Baja latencia

UNDO / REDO

Implementar Command Pattern.

Todas las operaciones deben ser reversibles:

* Cambio de parámetro
* Automatización
* Modulación
* Asignación de muestras
* Edición de loops
* Presets

COMPATIBILIDAD TUI

SeqTerm es un proyecto orientado a terminal.

Toda funcionalidad debe poder usarse desde:

* Terminal UI
* CLI
* GUI futura

Diseñar widgets abstractos.

No depender exclusivamente de GUI gráfica.

ARCHIVOS DE PROYECTO

Extender formato de proyecto para almacenar:

* Plugins
* Instrumentos
* Automatizaciones
* Modulaciones
* Presets embebidos
* Rutas MIDI Learn

DOCUMENTACIÓN

Generar:

* Diagrama de arquitectura
* Flujo de audio
* Flujo de eventos
* Flujo de automatización
* API pública

TESTING

Añadir:

* Unit Tests
* Integration Tests
* Stress Tests
* Real-Time Safety Tests

ENTREGABLE FINAL

Producir:

1. Diseño completo de arquitectura.
2. Refactorización necesaria del código actual.
3. Nuevas interfaces y clases.
4. Implementación de adaptadores.
5. Motor de automatización.
6. Motor de modulación.
7. Editor SF2 completo.
8. Base para SFZ.
9. Base para CLAP.
10. Código listo para compilar e integrar en SeqTerm.
