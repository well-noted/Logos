// SPDX-License-Identifier: GPL-3.0-or-later
package org.logos.bridge;

import java.io.IOException;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.nlogo.api.FormatterPair;
import org.nlogo.api.NetLogoLegacyDialect$;
import org.nlogo.api.Version$;
import org.nlogo.core.Model;
import org.nlogo.fileformat.ConversionError;
import org.nlogo.fileformat.ConversionResult;
import org.nlogo.fileformat.FailedConversionResult;
import org.nlogo.fileformat.FileFormat;
import org.nlogo.fileformat.NLogoAnyLoader;
import org.nlogo.fileformat.NLogoFormat;
import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.sdm.NLogoSDMFormat;
import org.nlogo.sdm.SDMAutoConvertable$;
import scala.reflect.ClassTag;
import scala.reflect.ClassTag$;
import scala.util.Try;

/**
 * {@code model.convertLegacy}: converts a {@code .nlogo} file to {@code .nlogox} the way
 * NetLogo 7.0.4's GUI does when it opens an old model and saves it (D-06).
 *
 * <p>The steps follow the GUI at {@code v7.0.4}: read with {@code FileFormat.standardAnyLoader}
 * plus the system dynamics serializer ({@code app/Adapters.scala},
 * {@code headless/HeadlessWorkspace.scala}); run {@code FileFormat.converter}, which rewrites
 * code written for older versions ({@code workspace/OpenModel.scala}); set the version to the
 * running NetLogo's ({@code app/ModelSaver.scala}, {@code currentModelInCurrentVersion}); write
 * with the {@code .nlogox} writer. The GUI registers its own system dynamics converter
 * ({@code SDMGuiAutoConvertable}); headless code uses the core one.
 */
final class Legacy {
    private Legacy() {}

    /** The result of a conversion: the warnings to show the user. */
    record Converted(String sourceVersion, List<String> warnings) {}

    static Map<String, Object> convert(Map<String, Object> params) throws RpcException {
        Path source = Path.of(Workspaces.string(params, "sourcePath"));
        Path output = Path.of(Workspaces.string(params, "outputPath"));
        String sourceName = source.getFileName() == null ? "" : source.getFileName().toString();
        String outputName = output.getFileName() == null ? "" : output.getFileName().toString();
        if (!sourceName.toLowerCase(java.util.Locale.ROOT).endsWith(".nlogo")) {
            throw new RpcException(RpcException.INVALID_PARAMS,
                    "sourcePath must be a .nlogo file (3D models are not supported yet)");
        }
        if (!outputName.toLowerCase(java.util.Locale.ROOT).endsWith(".nlogox")) {
            throw new RpcException(RpcException.INVALID_PARAMS, "outputPath must end in .nlogox");
        }
        if (Files.exists(output)) {
            throw new RpcException(RpcException.FILE_EXISTS, "output file exists: " + output,
                    Map.of("path", output.toString()));
        }
        Converted c = convert(source, output);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("outputPath", output.toString());
        result.put("sourceVersion", c.sourceVersion());
        result.put("warnings", new ArrayList<Object>(c.warnings()));
        return result;
    }

    static Converted convert(Path source, Path output) throws RpcException {
        HeadlessWorkspace ws = HeadlessWorkspace.newInstance();
        try {
            NLogoAnyLoader loader = loader(ws);
            Try<Model> read = loader.readModel(source.toUri());
            if (read.isFailure()) {
                Throwable t = read.failed().get();
                throw new RpcException(RpcException.MODEL_OPEN_FAILED,
                        "cannot read " + source + ": " + t.getMessage(),
                        Map.of("path", source.toString(), "cause", String.valueOf(t)));
            }
            Model original = read.get();
            if (!original.version().startsWith("NetLogo")) {
                throw new RpcException(RpcException.MODEL_OPEN_FAILED,
                        "not a NetLogo model (version \"" + original.version() + "\")",
                        Map.of("path", source.toString(), "cause", "unknown version line"));
            }
            if (original.version().startsWith("NetLogo 3D")) {
                throw new RpcException(RpcException.MODEL_OPEN_FAILED,
                        "3D models are not supported yet",
                        Map.of("path", source.toString(), "cause", "3D model"));
            }

            List<String> warnings = new ArrayList<>();
            ConversionResult converted = FileFormat.converter(
                    ws.getExtensionManager(), ws.getLibraryManager(),
                    ws.getCompilationEnvironment(), ws, autoConvertables(),
                    NetLogoLegacyDialect$.MODULE$).apply(original, source);
            if (converted instanceof FailedConversionResult failed) {
                // The GUI shows these errors and, if the user continues, opens the partly
                // converted model.
                scala.collection.Iterator<ConversionError> it = failed.errors().iterator();
                while (it.hasNext()) {
                    ConversionError e = it.next();
                    StringBuilder msg = new StringBuilder(e.componentDescription())
                            .append(": ").append(e.conversionDescription());
                    scala.collection.Iterator<Exception> ex = e.errors().iterator();
                    while (ex.hasNext()) {
                        msg.append("; ").append(ex.next().getMessage());
                    }
                    warnings.add(msg.toString());
                }
            }
            Model m = converted.model();
            Model current = new Model(m.code(), m.widgets(), m.info(),
                    Version$.MODULE$.version(), m.turtleShapes(), m.linkShapes(),
                    m.optionalSections(), m.resources(), m.unknownSections());

            Try<String> text = loader.sourceString(current, "nlogox");
            if (text.isFailure()) {
                throw new RpcException(RpcException.INTERNAL,
                        "cannot write the model: " + text.failed().get().getMessage());
            }
            // CREATE_NEW: never replace an existing file, even one created since the check.
            try (Writer w = Files.newBufferedWriter(output, StandardCharsets.UTF_8,
                    StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)) {
                w.write(text.get());
            } catch (FileAlreadyExistsException e) {
                throw new RpcException(RpcException.FILE_EXISTS, "output file exists: " + output,
                        Map.of("path", output.toString()));
            } catch (IOException e) {
                throw new RpcException(RpcException.MODEL_OPEN_FAILED,
                        "cannot write " + output + ": " + e.getMessage(),
                        Map.of("path", output.toString(), "cause", String.valueOf(e)));
            }
            return new Converted(original.version(), warnings);
        } finally {
            try {
                ws.dispose();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }
    }

    /** {@code FileFormat.standardAnyLoader} with the system dynamics serializer added. */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static NLogoAnyLoader loader(HeadlessWorkspace ws) {
        ClassTag<String[]> lines = ClassTag$.MODULE$.apply(String[].class);
        ClassTag<NLogoFormat> format = ClassTag$.MODULE$.apply(NLogoFormat.class);
        ClassTag pair = ClassTag$.MODULE$.apply(FormatterPair.class);
        return FileFormat.standardAnyLoader(true, ws, true)
                .addSerializer(new NLogoSDMFormat(), lines, format, pair);
    }

    private static scala.collection.immutable.Seq<org.nlogo.api.AutoConvertable>
            autoConvertables() {
        List<org.nlogo.api.AutoConvertable> all = new ArrayList<>(
                scala.jdk.javaapi.CollectionConverters.asJava(FileFormat.defaultAutoConvertables()));
        all.add(SDMAutoConvertable$.MODULE$);
        return scala.jdk.javaapi.CollectionConverters.asScala(all).toList();
    }
}
