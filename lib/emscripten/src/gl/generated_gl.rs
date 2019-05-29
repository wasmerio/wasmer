pub mod gl_static {

    #![allow(unused_imports)]
    use gl::{self, types};
    mod __gl_imports {
        pub use std::mem;
        pub use std::os::raw;
    }

    #[allow(non_snake_case, unused_variables, dead_code)]
    pub mod wasmer {
        use super::__gl_imports;
        use gl::{self, types};
        use wasmer_runtime_core::vm::Ctx;
        use wasmer_runtime_core::{func, import::ImportObject, imports};
        pub fn gl_imports() -> ImportObject {
            imports! {
            "env" => {
            "_emscripten_glActiveShaderProgram" => func!(_glActiveShaderProgram),
            "_glActiveShaderProgram" => func!(_glActiveShaderProgram),
            "_emscripten_glActiveTexture" => func!(_glActiveTexture),
            "_glActiveTexture" => func!(_glActiveTexture),
            "_emscripten_glAttachShader" => func!(_glAttachShader),
            "_glAttachShader" => func!(_glAttachShader),
            "_emscripten_glBeginConditionalRender" => func!(_glBeginConditionalRender),
            "_glBeginConditionalRender" => func!(_glBeginConditionalRender),
            "_emscripten_glBeginQuery" => func!(_glBeginQuery),
            "_glBeginQuery" => func!(_glBeginQuery),
            "_emscripten_glBeginQueryIndexed" => func!(_glBeginQueryIndexed),
            "_glBeginQueryIndexed" => func!(_glBeginQueryIndexed),
            "_emscripten_glBeginTransformFeedback" => func!(_glBeginTransformFeedback),
            "_glBeginTransformFeedback" => func!(_glBeginTransformFeedback),
            "_emscripten_glBindAttribLocation" => func!(_glBindAttribLocation),
            "_glBindAttribLocation" => func!(_glBindAttribLocation),
            "_emscripten_glBindBuffer" => func!(_glBindBuffer),
            "_glBindBuffer" => func!(_glBindBuffer),
            "_emscripten_glBindBufferBase" => func!(_glBindBufferBase),
            "_glBindBufferBase" => func!(_glBindBufferBase),
            "_emscripten_glBindBufferRange" => func!(_glBindBufferRange),
            "_glBindBufferRange" => func!(_glBindBufferRange),
            "_emscripten_glBindBuffersBase" => func!(_glBindBuffersBase),
            "_glBindBuffersBase" => func!(_glBindBuffersBase),
            "_emscripten_glBindBuffersRange" => func!(_glBindBuffersRange),
            "_glBindBuffersRange" => func!(_glBindBuffersRange),
            "_emscripten_glBindFragDataLocation" => func!(_glBindFragDataLocation),
            "_glBindFragDataLocation" => func!(_glBindFragDataLocation),
            "_emscripten_glBindFragDataLocationIndexed" => func!(_glBindFragDataLocationIndexed),
            "_glBindFragDataLocationIndexed" => func!(_glBindFragDataLocationIndexed),
            "_emscripten_glBindFramebuffer" => func!(_glBindFramebuffer),
            "_glBindFramebuffer" => func!(_glBindFramebuffer),
            "_emscripten_glBindImageTexture" => func!(_glBindImageTexture),
            "_glBindImageTexture" => func!(_glBindImageTexture),
            "_emscripten_glBindImageTextures" => func!(_glBindImageTextures),
            "_glBindImageTextures" => func!(_glBindImageTextures),
            "_emscripten_glBindProgramPipeline" => func!(_glBindProgramPipeline),
            "_glBindProgramPipeline" => func!(_glBindProgramPipeline),
            "_emscripten_glBindRenderbuffer" => func!(_glBindRenderbuffer),
            "_glBindRenderbuffer" => func!(_glBindRenderbuffer),
            "_emscripten_glBindSampler" => func!(_glBindSampler),
            "_glBindSampler" => func!(_glBindSampler),
            "_emscripten_glBindSamplers" => func!(_glBindSamplers),
            "_glBindSamplers" => func!(_glBindSamplers),
            "_emscripten_glBindTexture" => func!(_glBindTexture),
            "_glBindTexture" => func!(_glBindTexture),
            "_emscripten_glBindTextureUnit" => func!(_glBindTextureUnit),
            "_glBindTextureUnit" => func!(_glBindTextureUnit),
            "_emscripten_glBindTextures" => func!(_glBindTextures),
            "_glBindTextures" => func!(_glBindTextures),
            "_emscripten_glBindTransformFeedback" => func!(_glBindTransformFeedback),
            "_glBindTransformFeedback" => func!(_glBindTransformFeedback),
            "_emscripten_glBindVertexArray" => func!(_glBindVertexArray),
            "_glBindVertexArray" => func!(_glBindVertexArray),
            "_emscripten_glBindVertexBuffer" => func!(_glBindVertexBuffer),
            "_glBindVertexBuffer" => func!(_glBindVertexBuffer),
            "_emscripten_glBindVertexBuffers" => func!(_glBindVertexBuffers),
            "_glBindVertexBuffers" => func!(_glBindVertexBuffers),
            "_emscripten_glBlendColor" => func!(_glBlendColor),
            "_glBlendColor" => func!(_glBlendColor),
            "_emscripten_glBlendEquation" => func!(_glBlendEquation),
            "_glBlendEquation" => func!(_glBlendEquation),
            "_emscripten_glBlendEquationSeparate" => func!(_glBlendEquationSeparate),
            "_glBlendEquationSeparate" => func!(_glBlendEquationSeparate),
            "_emscripten_glBlendEquationSeparatei" => func!(_glBlendEquationSeparatei),
            "_glBlendEquationSeparatei" => func!(_glBlendEquationSeparatei),
            "_emscripten_glBlendEquationi" => func!(_glBlendEquationi),
            "_glBlendEquationi" => func!(_glBlendEquationi),
            "_emscripten_glBlendFunc" => func!(_glBlendFunc),
            "_glBlendFunc" => func!(_glBlendFunc),
            "_emscripten_glBlendFuncSeparate" => func!(_glBlendFuncSeparate),
            "_glBlendFuncSeparate" => func!(_glBlendFuncSeparate),
            "_emscripten_glBlendFuncSeparatei" => func!(_glBlendFuncSeparatei),
            "_glBlendFuncSeparatei" => func!(_glBlendFuncSeparatei),
            "_emscripten_glBlendFunci" => func!(_glBlendFunci),
            "_glBlendFunci" => func!(_glBlendFunci),
            "_emscripten_glBlitFramebuffer" => func!(_glBlitFramebuffer),
            "_glBlitFramebuffer" => func!(_glBlitFramebuffer),
            "_emscripten_glBlitNamedFramebuffer" => func!(_glBlitNamedFramebuffer),
            "_glBlitNamedFramebuffer" => func!(_glBlitNamedFramebuffer),
            "_emscripten_glBufferData" => func!(_glBufferData),
            "_glBufferData" => func!(_glBufferData),
            "_emscripten_glBufferStorage" => func!(_glBufferStorage),
            "_glBufferStorage" => func!(_glBufferStorage),
            "_emscripten_glBufferSubData" => func!(_glBufferSubData),
            "_glBufferSubData" => func!(_glBufferSubData),
            "_emscripten_glCheckFramebufferStatus" => func!(_glCheckFramebufferStatus),
            "_glCheckFramebufferStatus" => func!(_glCheckFramebufferStatus),
            "_emscripten_glCheckNamedFramebufferStatus" => func!(_glCheckNamedFramebufferStatus),
            "_glCheckNamedFramebufferStatus" => func!(_glCheckNamedFramebufferStatus),
            "_emscripten_glClampColor" => func!(_glClampColor),
            "_glClampColor" => func!(_glClampColor),
            "_emscripten_glClear" => func!(_glClear),
            "_glClear" => func!(_glClear),
            "_emscripten_glClearBufferData" => func!(_glClearBufferData),
            "_glClearBufferData" => func!(_glClearBufferData),
            "_emscripten_glClearBufferSubData" => func!(_glClearBufferSubData),
            "_glClearBufferSubData" => func!(_glClearBufferSubData),
            "_emscripten_glClearBufferfi" => func!(_glClearBufferfi),
            "_glClearBufferfi" => func!(_glClearBufferfi),
            "_emscripten_glClearBufferfv" => func!(_glClearBufferfv),
            "_glClearBufferfv" => func!(_glClearBufferfv),
            "_emscripten_glClearBufferiv" => func!(_glClearBufferiv),
            "_glClearBufferiv" => func!(_glClearBufferiv),
            "_emscripten_glClearBufferuiv" => func!(_glClearBufferuiv),
            "_glClearBufferuiv" => func!(_glClearBufferuiv),
            "_emscripten_glClearColor" => func!(_glClearColor),
            "_glClearColor" => func!(_glClearColor),
            "_emscripten_glClearDepth" => func!(_glClearDepth),
            "_glClearDepth" => func!(_glClearDepth),
            "_emscripten_glClearDepthf" => func!(_glClearDepthf),
            "_glClearDepthf" => func!(_glClearDepthf),
            "_emscripten_glClearNamedBufferData" => func!(_glClearNamedBufferData),
            "_glClearNamedBufferData" => func!(_glClearNamedBufferData),
            "_emscripten_glClearNamedBufferSubData" => func!(_glClearNamedBufferSubData),
            "_glClearNamedBufferSubData" => func!(_glClearNamedBufferSubData),
            "_emscripten_glClearNamedFramebufferfi" => func!(_glClearNamedFramebufferfi),
            "_glClearNamedFramebufferfi" => func!(_glClearNamedFramebufferfi),
            "_emscripten_glClearNamedFramebufferfv" => func!(_glClearNamedFramebufferfv),
            "_glClearNamedFramebufferfv" => func!(_glClearNamedFramebufferfv),
            "_emscripten_glClearNamedFramebufferiv" => func!(_glClearNamedFramebufferiv),
            "_glClearNamedFramebufferiv" => func!(_glClearNamedFramebufferiv),
            "_emscripten_glClearNamedFramebufferuiv" => func!(_glClearNamedFramebufferuiv),
            "_glClearNamedFramebufferuiv" => func!(_glClearNamedFramebufferuiv),
            "_emscripten_glClearStencil" => func!(_glClearStencil),
            "_glClearStencil" => func!(_glClearStencil),
            "_emscripten_glClearTexImage" => func!(_glClearTexImage),
            "_glClearTexImage" => func!(_glClearTexImage),
            "_emscripten_glClearTexSubImage" => func!(_glClearTexSubImage),
            "_glClearTexSubImage" => func!(_glClearTexSubImage),
            "_emscripten_glClientWaitSync" => func!(_glClientWaitSync),
            "_glClientWaitSync" => func!(_glClientWaitSync),
            "_emscripten_glClipControl" => func!(_glClipControl),
            "_glClipControl" => func!(_glClipControl),
            "_emscripten_glColorMask" => func!(_glColorMask),
            "_glColorMask" => func!(_glColorMask),
            "_emscripten_glColorMaski" => func!(_glColorMaski),
            "_glColorMaski" => func!(_glColorMaski),
            "_emscripten_glColorP3ui" => func!(_glColorP3ui),
            "_glColorP3ui" => func!(_glColorP3ui),
            "_emscripten_glColorP3uiv" => func!(_glColorP3uiv),
            "_glColorP3uiv" => func!(_glColorP3uiv),
            "_emscripten_glColorP4ui" => func!(_glColorP4ui),
            "_glColorP4ui" => func!(_glColorP4ui),
            "_emscripten_glColorP4uiv" => func!(_glColorP4uiv),
            "_glColorP4uiv" => func!(_glColorP4uiv),
            "_emscripten_glCompileShader" => func!(_glCompileShader),
            "_glCompileShader" => func!(_glCompileShader),
            "_emscripten_glCompressedTexImage1D" => func!(_glCompressedTexImage1D),
            "_glCompressedTexImage1D" => func!(_glCompressedTexImage1D),
            "_emscripten_glCompressedTexImage2D" => func!(_glCompressedTexImage2D),
            "_glCompressedTexImage2D" => func!(_glCompressedTexImage2D),
            "_emscripten_glCompressedTexImage3D" => func!(_glCompressedTexImage3D),
            "_glCompressedTexImage3D" => func!(_glCompressedTexImage3D),
            "_emscripten_glCompressedTexSubImage1D" => func!(_glCompressedTexSubImage1D),
            "_glCompressedTexSubImage1D" => func!(_glCompressedTexSubImage1D),
            "_emscripten_glCompressedTexSubImage2D" => func!(_glCompressedTexSubImage2D),
            "_glCompressedTexSubImage2D" => func!(_glCompressedTexSubImage2D),
            "_emscripten_glCompressedTexSubImage3D" => func!(_glCompressedTexSubImage3D),
            "_glCompressedTexSubImage3D" => func!(_glCompressedTexSubImage3D),
            "_emscripten_glCompressedTextureSubImage1D" => func!(_glCompressedTextureSubImage1D),
            "_glCompressedTextureSubImage1D" => func!(_glCompressedTextureSubImage1D),
            "_emscripten_glCompressedTextureSubImage2D" => func!(_glCompressedTextureSubImage2D),
            "_glCompressedTextureSubImage2D" => func!(_glCompressedTextureSubImage2D),
            "_emscripten_glCompressedTextureSubImage3D" => func!(_glCompressedTextureSubImage3D),
            "_glCompressedTextureSubImage3D" => func!(_glCompressedTextureSubImage3D),
            "_emscripten_glCopyBufferSubData" => func!(_glCopyBufferSubData),
            "_glCopyBufferSubData" => func!(_glCopyBufferSubData),
            //"_emscripten_glCopyImageSubData" => func!(_glCopyImageSubData),
            //"_glCopyImageSubData" => func!(_glCopyImageSubData),
            "_emscripten_glCopyNamedBufferSubData" => func!(_glCopyNamedBufferSubData),
            "_glCopyNamedBufferSubData" => func!(_glCopyNamedBufferSubData),
            "_emscripten_glCopyTexImage1D" => func!(_glCopyTexImage1D),
            "_glCopyTexImage1D" => func!(_glCopyTexImage1D),
            "_emscripten_glCopyTexImage2D" => func!(_glCopyTexImage2D),
            "_glCopyTexImage2D" => func!(_glCopyTexImage2D),
            "_emscripten_glCopyTexSubImage1D" => func!(_glCopyTexSubImage1D),
            "_glCopyTexSubImage1D" => func!(_glCopyTexSubImage1D),
            "_emscripten_glCopyTexSubImage2D" => func!(_glCopyTexSubImage2D),
            "_glCopyTexSubImage2D" => func!(_glCopyTexSubImage2D),
            "_emscripten_glCopyTexSubImage3D" => func!(_glCopyTexSubImage3D),
            "_glCopyTexSubImage3D" => func!(_glCopyTexSubImage3D),
            "_emscripten_glCopyTextureSubImage1D" => func!(_glCopyTextureSubImage1D),
            "_glCopyTextureSubImage1D" => func!(_glCopyTextureSubImage1D),
            "_emscripten_glCopyTextureSubImage2D" => func!(_glCopyTextureSubImage2D),
            "_glCopyTextureSubImage2D" => func!(_glCopyTextureSubImage2D),
            "_emscripten_glCopyTextureSubImage3D" => func!(_glCopyTextureSubImage3D),
            "_glCopyTextureSubImage3D" => func!(_glCopyTextureSubImage3D),
            "_emscripten_glCreateBuffers" => func!(_glCreateBuffers),
            "_glCreateBuffers" => func!(_glCreateBuffers),
            "_emscripten_glCreateFramebuffers" => func!(_glCreateFramebuffers),
            "_glCreateFramebuffers" => func!(_glCreateFramebuffers),
            "_emscripten_glCreateProgram" => func!(_glCreateProgram),
            "_glCreateProgram" => func!(_glCreateProgram),
            "_emscripten_glCreateProgramPipelines" => func!(_glCreateProgramPipelines),
            "_glCreateProgramPipelines" => func!(_glCreateProgramPipelines),
            "_emscripten_glCreateQueries" => func!(_glCreateQueries),
            "_glCreateQueries" => func!(_glCreateQueries),
            "_emscripten_glCreateRenderbuffers" => func!(_glCreateRenderbuffers),
            "_glCreateRenderbuffers" => func!(_glCreateRenderbuffers),
            "_emscripten_glCreateSamplers" => func!(_glCreateSamplers),
            "_glCreateSamplers" => func!(_glCreateSamplers),
            "_emscripten_glCreateShader" => func!(_glCreateShader),
            "_glCreateShader" => func!(_glCreateShader),
            "_emscripten_glCreateShaderProgramv" => func!(_glCreateShaderProgramv),
            "_glCreateShaderProgramv" => func!(_glCreateShaderProgramv),
            "_emscripten_glCreateTextures" => func!(_glCreateTextures),
            "_glCreateTextures" => func!(_glCreateTextures),
            "_emscripten_glCreateTransformFeedbacks" => func!(_glCreateTransformFeedbacks),
            "_glCreateTransformFeedbacks" => func!(_glCreateTransformFeedbacks),
            "_emscripten_glCreateVertexArrays" => func!(_glCreateVertexArrays),
            "_glCreateVertexArrays" => func!(_glCreateVertexArrays),
            "_emscripten_glCullFace" => func!(_glCullFace),
            "_glCullFace" => func!(_glCullFace),
            //"_emscripten_glDebugMessageCallback" => func!(_glDebugMessageCallback),
            //"_glDebugMessageCallback" => func!(_glDebugMessageCallback),
            "_emscripten_glDebugMessageControl" => func!(_glDebugMessageControl),
            "_glDebugMessageControl" => func!(_glDebugMessageControl),
            "_emscripten_glDebugMessageInsert" => func!(_glDebugMessageInsert),
            "_glDebugMessageInsert" => func!(_glDebugMessageInsert),
            "_emscripten_glDeleteBuffers" => func!(_glDeleteBuffers),
            "_glDeleteBuffers" => func!(_glDeleteBuffers),
            "_emscripten_glDeleteFramebuffers" => func!(_glDeleteFramebuffers),
            "_glDeleteFramebuffers" => func!(_glDeleteFramebuffers),
            "_emscripten_glDeleteProgram" => func!(_glDeleteProgram),
            "_glDeleteProgram" => func!(_glDeleteProgram),
            "_emscripten_glDeleteProgramPipelines" => func!(_glDeleteProgramPipelines),
            "_glDeleteProgramPipelines" => func!(_glDeleteProgramPipelines),
            "_emscripten_glDeleteQueries" => func!(_glDeleteQueries),
            "_glDeleteQueries" => func!(_glDeleteQueries),
            "_emscripten_glDeleteRenderbuffers" => func!(_glDeleteRenderbuffers),
            "_glDeleteRenderbuffers" => func!(_glDeleteRenderbuffers),
            "_emscripten_glDeleteSamplers" => func!(_glDeleteSamplers),
            "_glDeleteSamplers" => func!(_glDeleteSamplers),
            "_emscripten_glDeleteShader" => func!(_glDeleteShader),
            "_glDeleteShader" => func!(_glDeleteShader),
            "_emscripten_glDeleteSync" => func!(_glDeleteSync),
            "_glDeleteSync" => func!(_glDeleteSync),
            "_emscripten_glDeleteTextures" => func!(_glDeleteTextures),
            "_glDeleteTextures" => func!(_glDeleteTextures),
            "_emscripten_glDeleteTransformFeedbacks" => func!(_glDeleteTransformFeedbacks),
            "_glDeleteTransformFeedbacks" => func!(_glDeleteTransformFeedbacks),
            "_emscripten_glDeleteVertexArrays" => func!(_glDeleteVertexArrays),
            "_glDeleteVertexArrays" => func!(_glDeleteVertexArrays),
            "_emscripten_glDepthFunc" => func!(_glDepthFunc),
            "_glDepthFunc" => func!(_glDepthFunc),
            "_emscripten_glDepthMask" => func!(_glDepthMask),
            "_glDepthMask" => func!(_glDepthMask),
            "_emscripten_glDepthRange" => func!(_glDepthRange),
            "_glDepthRange" => func!(_glDepthRange),
            "_emscripten_glDepthRangeArrayv" => func!(_glDepthRangeArrayv),
            "_glDepthRangeArrayv" => func!(_glDepthRangeArrayv),
            "_emscripten_glDepthRangeIndexed" => func!(_glDepthRangeIndexed),
            "_glDepthRangeIndexed" => func!(_glDepthRangeIndexed),
            "_emscripten_glDepthRangef" => func!(_glDepthRangef),
            "_glDepthRangef" => func!(_glDepthRangef),
            "_emscripten_glDetachShader" => func!(_glDetachShader),
            "_glDetachShader" => func!(_glDetachShader),
            "_emscripten_glDisable" => func!(_glDisable),
            "_glDisable" => func!(_glDisable),
            "_emscripten_glDisableVertexArrayAttrib" => func!(_glDisableVertexArrayAttrib),
            "_glDisableVertexArrayAttrib" => func!(_glDisableVertexArrayAttrib),
            "_emscripten_glDisableVertexAttribArray" => func!(_glDisableVertexAttribArray),
            "_glDisableVertexAttribArray" => func!(_glDisableVertexAttribArray),
            "_emscripten_glDisablei" => func!(_glDisablei),
            "_glDisablei" => func!(_glDisablei),
            "_emscripten_glDispatchCompute" => func!(_glDispatchCompute),
            "_glDispatchCompute" => func!(_glDispatchCompute),
            "_emscripten_glDispatchComputeIndirect" => func!(_glDispatchComputeIndirect),
            "_glDispatchComputeIndirect" => func!(_glDispatchComputeIndirect),
            "_emscripten_glDrawArrays" => func!(_glDrawArrays),
            "_glDrawArrays" => func!(_glDrawArrays),
            "_emscripten_glDrawArraysIndirect" => func!(_glDrawArraysIndirect),
            "_glDrawArraysIndirect" => func!(_glDrawArraysIndirect),
            "_emscripten_glDrawArraysInstanced" => func!(_glDrawArraysInstanced),
            "_glDrawArraysInstanced" => func!(_glDrawArraysInstanced),
            "_emscripten_glDrawArraysInstancedBaseInstance" => func!(_glDrawArraysInstancedBaseInstance),
            "_glDrawArraysInstancedBaseInstance" => func!(_glDrawArraysInstancedBaseInstance),
            "_emscripten_glDrawBuffer" => func!(_glDrawBuffer),
            "_glDrawBuffer" => func!(_glDrawBuffer),
            "_emscripten_glDrawBuffers" => func!(_glDrawBuffers),
            "_glDrawBuffers" => func!(_glDrawBuffers),
            "_emscripten_glDrawElements" => func!(_glDrawElements),
            "_glDrawElements" => func!(_glDrawElements),
            "_emscripten_glDrawElementsBaseVertex" => func!(_glDrawElementsBaseVertex),
            "_glDrawElementsBaseVertex" => func!(_glDrawElementsBaseVertex),
            "_emscripten_glDrawElementsIndirect" => func!(_glDrawElementsIndirect),
            "_glDrawElementsIndirect" => func!(_glDrawElementsIndirect),
            "_emscripten_glDrawElementsInstanced" => func!(_glDrawElementsInstanced),
            "_glDrawElementsInstanced" => func!(_glDrawElementsInstanced),
            "_emscripten_glDrawElementsInstancedBaseInstance" => func!(_glDrawElementsInstancedBaseInstance),
            "_glDrawElementsInstancedBaseInstance" => func!(_glDrawElementsInstancedBaseInstance),
            "_emscripten_glDrawElementsInstancedBaseVertex" => func!(_glDrawElementsInstancedBaseVertex),
            "_glDrawElementsInstancedBaseVertex" => func!(_glDrawElementsInstancedBaseVertex),
            "_emscripten_glDrawElementsInstancedBaseVertexBaseInstance" => func!(_glDrawElementsInstancedBaseVertexBaseInstance),
            "_glDrawElementsInstancedBaseVertexBaseInstance" => func!(_glDrawElementsInstancedBaseVertexBaseInstance),
            "_emscripten_glDrawRangeElements" => func!(_glDrawRangeElements),
            "_glDrawRangeElements" => func!(_glDrawRangeElements),
            "_emscripten_glDrawRangeElementsBaseVertex" => func!(_glDrawRangeElementsBaseVertex),
            "_glDrawRangeElementsBaseVertex" => func!(_glDrawRangeElementsBaseVertex),
            "_emscripten_glDrawTransformFeedback" => func!(_glDrawTransformFeedback),
            "_glDrawTransformFeedback" => func!(_glDrawTransformFeedback),
            "_emscripten_glDrawTransformFeedbackInstanced" => func!(_glDrawTransformFeedbackInstanced),
            "_glDrawTransformFeedbackInstanced" => func!(_glDrawTransformFeedbackInstanced),
            "_emscripten_glDrawTransformFeedbackStream" => func!(_glDrawTransformFeedbackStream),
            "_glDrawTransformFeedbackStream" => func!(_glDrawTransformFeedbackStream),
            "_emscripten_glDrawTransformFeedbackStreamInstanced" => func!(_glDrawTransformFeedbackStreamInstanced),
            "_glDrawTransformFeedbackStreamInstanced" => func!(_glDrawTransformFeedbackStreamInstanced),
            "_emscripten_glEnable" => func!(_glEnable),
            "_glEnable" => func!(_glEnable),
            "_emscripten_glEnableVertexArrayAttrib" => func!(_glEnableVertexArrayAttrib),
            "_glEnableVertexArrayAttrib" => func!(_glEnableVertexArrayAttrib),
            "_emscripten_glEnableVertexAttribArray" => func!(_glEnableVertexAttribArray),
            "_glEnableVertexAttribArray" => func!(_glEnableVertexAttribArray),
            "_emscripten_glEnablei" => func!(_glEnablei),
            "_glEnablei" => func!(_glEnablei),
            "_emscripten_glEndConditionalRender" => func!(_glEndConditionalRender),
            "_glEndConditionalRender" => func!(_glEndConditionalRender),
            "_emscripten_glEndQuery" => func!(_glEndQuery),
            "_glEndQuery" => func!(_glEndQuery),
            "_emscripten_glEndQueryIndexed" => func!(_glEndQueryIndexed),
            "_glEndQueryIndexed" => func!(_glEndQueryIndexed),
            "_emscripten_glEndTransformFeedback" => func!(_glEndTransformFeedback),
            "_glEndTransformFeedback" => func!(_glEndTransformFeedback),
            "_emscripten_glFenceSync" => func!(_glFenceSync),
            "_glFenceSync" => func!(_glFenceSync),
            "_emscripten_glFinish" => func!(_glFinish),
            "_glFinish" => func!(_glFinish),
            "_emscripten_glFlush" => func!(_glFlush),
            "_glFlush" => func!(_glFlush),
            "_emscripten_glFlushMappedBufferRange" => func!(_glFlushMappedBufferRange),
            "_glFlushMappedBufferRange" => func!(_glFlushMappedBufferRange),
            "_emscripten_glFlushMappedNamedBufferRange" => func!(_glFlushMappedNamedBufferRange),
            "_glFlushMappedNamedBufferRange" => func!(_glFlushMappedNamedBufferRange),
            "_emscripten_glFramebufferParameteri" => func!(_glFramebufferParameteri),
            "_glFramebufferParameteri" => func!(_glFramebufferParameteri),
            "_emscripten_glFramebufferRenderbuffer" => func!(_glFramebufferRenderbuffer),
            "_glFramebufferRenderbuffer" => func!(_glFramebufferRenderbuffer),
            "_emscripten_glFramebufferTexture" => func!(_glFramebufferTexture),
            "_glFramebufferTexture" => func!(_glFramebufferTexture),
            "_emscripten_glFramebufferTexture1D" => func!(_glFramebufferTexture1D),
            "_glFramebufferTexture1D" => func!(_glFramebufferTexture1D),
            "_emscripten_glFramebufferTexture2D" => func!(_glFramebufferTexture2D),
            "_glFramebufferTexture2D" => func!(_glFramebufferTexture2D),
            "_emscripten_glFramebufferTexture3D" => func!(_glFramebufferTexture3D),
            "_glFramebufferTexture3D" => func!(_glFramebufferTexture3D),
            "_emscripten_glFramebufferTextureLayer" => func!(_glFramebufferTextureLayer),
            "_glFramebufferTextureLayer" => func!(_glFramebufferTextureLayer),
            "_emscripten_glFrontFace" => func!(_glFrontFace),
            "_glFrontFace" => func!(_glFrontFace),
            "_emscripten_glGenBuffers" => func!(_glGenBuffers),
            "_glGenBuffers" => func!(_glGenBuffers),
            "_emscripten_glGenFramebuffers" => func!(_glGenFramebuffers),
            "_glGenFramebuffers" => func!(_glGenFramebuffers),
            "_emscripten_glGenProgramPipelines" => func!(_glGenProgramPipelines),
            "_glGenProgramPipelines" => func!(_glGenProgramPipelines),
            "_emscripten_glGenQueries" => func!(_glGenQueries),
            "_glGenQueries" => func!(_glGenQueries),
            "_emscripten_glGenRenderbuffers" => func!(_glGenRenderbuffers),
            "_glGenRenderbuffers" => func!(_glGenRenderbuffers),
            "_emscripten_glGenSamplers" => func!(_glGenSamplers),
            "_glGenSamplers" => func!(_glGenSamplers),
            "_emscripten_glGenTextures" => func!(_glGenTextures),
            "_glGenTextures" => func!(_glGenTextures),
            "_emscripten_glGenTransformFeedbacks" => func!(_glGenTransformFeedbacks),
            "_glGenTransformFeedbacks" => func!(_glGenTransformFeedbacks),
            "_emscripten_glGenVertexArrays" => func!(_glGenVertexArrays),
            "_glGenVertexArrays" => func!(_glGenVertexArrays),
            "_emscripten_glGenerateMipmap" => func!(_glGenerateMipmap),
            "_glGenerateMipmap" => func!(_glGenerateMipmap),
            "_emscripten_glGenerateTextureMipmap" => func!(_glGenerateTextureMipmap),
            "_glGenerateTextureMipmap" => func!(_glGenerateTextureMipmap),
            "_emscripten_glGetActiveAtomicCounterBufferiv" => func!(_glGetActiveAtomicCounterBufferiv),
            "_glGetActiveAtomicCounterBufferiv" => func!(_glGetActiveAtomicCounterBufferiv),
            "_emscripten_glGetActiveAttrib" => func!(_glGetActiveAttrib),
            "_glGetActiveAttrib" => func!(_glGetActiveAttrib),
            "_emscripten_glGetActiveSubroutineName" => func!(_glGetActiveSubroutineName),
            "_glGetActiveSubroutineName" => func!(_glGetActiveSubroutineName),
            "_emscripten_glGetActiveSubroutineUniformName" => func!(_glGetActiveSubroutineUniformName),
            "_glGetActiveSubroutineUniformName" => func!(_glGetActiveSubroutineUniformName),
            "_emscripten_glGetActiveSubroutineUniformiv" => func!(_glGetActiveSubroutineUniformiv),
            "_glGetActiveSubroutineUniformiv" => func!(_glGetActiveSubroutineUniformiv),
            "_emscripten_glGetActiveUniform" => func!(_glGetActiveUniform),
            "_glGetActiveUniform" => func!(_glGetActiveUniform),
            "_emscripten_glGetActiveUniformBlockName" => func!(_glGetActiveUniformBlockName),
            "_glGetActiveUniformBlockName" => func!(_glGetActiveUniformBlockName),
            "_emscripten_glGetActiveUniformBlockiv" => func!(_glGetActiveUniformBlockiv),
            "_glGetActiveUniformBlockiv" => func!(_glGetActiveUniformBlockiv),
            "_emscripten_glGetActiveUniformName" => func!(_glGetActiveUniformName),
            "_glGetActiveUniformName" => func!(_glGetActiveUniformName),
            "_emscripten_glGetActiveUniformsiv" => func!(_glGetActiveUniformsiv),
            "_glGetActiveUniformsiv" => func!(_glGetActiveUniformsiv),
            "_emscripten_glGetAttachedShaders" => func!(_glGetAttachedShaders),
            "_glGetAttachedShaders" => func!(_glGetAttachedShaders),
            "_emscripten_glGetAttribLocation" => func!(_glGetAttribLocation),
            "_glGetAttribLocation" => func!(_glGetAttribLocation),
            "_emscripten_glGetBooleani_v" => func!(_glGetBooleani_v),
            "_glGetBooleani_v" => func!(_glGetBooleani_v),
            "_emscripten_glGetBooleanv" => func!(_glGetBooleanv),
            "_glGetBooleanv" => func!(_glGetBooleanv),
            "_emscripten_glGetBufferParameteri64v" => func!(_glGetBufferParameteri64v),
            "_glGetBufferParameteri64v" => func!(_glGetBufferParameteri64v),
            "_emscripten_glGetBufferParameteriv" => func!(_glGetBufferParameteriv),
            "_glGetBufferParameteriv" => func!(_glGetBufferParameteriv),
            "_emscripten_glGetBufferPointerv" => func!(_glGetBufferPointerv),
            "_glGetBufferPointerv" => func!(_glGetBufferPointerv),
            "_emscripten_glGetBufferSubData" => func!(_glGetBufferSubData),
            "_glGetBufferSubData" => func!(_glGetBufferSubData),
            "_emscripten_glGetCompressedTexImage" => func!(_glGetCompressedTexImage),
            "_glGetCompressedTexImage" => func!(_glGetCompressedTexImage),
            "_emscripten_glGetCompressedTextureImage" => func!(_glGetCompressedTextureImage),
            "_glGetCompressedTextureImage" => func!(_glGetCompressedTextureImage),
            "_emscripten_glGetCompressedTextureSubImage" => func!(_glGetCompressedTextureSubImage),
            "_glGetCompressedTextureSubImage" => func!(_glGetCompressedTextureSubImage),
            "_emscripten_glGetDebugMessageLog" => func!(_glGetDebugMessageLog),
            "_glGetDebugMessageLog" => func!(_glGetDebugMessageLog),
            "_emscripten_glGetDoublei_v" => func!(_glGetDoublei_v),
            "_glGetDoublei_v" => func!(_glGetDoublei_v),
            "_emscripten_glGetDoublev" => func!(_glGetDoublev),
            "_glGetDoublev" => func!(_glGetDoublev),
            "_emscripten_glGetError" => func!(_glGetError),
            "_glGetError" => func!(_glGetError),
            "_emscripten_glGetFloati_v" => func!(_glGetFloati_v),
            "_glGetFloati_v" => func!(_glGetFloati_v),
            "_emscripten_glGetFloatv" => func!(_glGetFloatv),
            "_glGetFloatv" => func!(_glGetFloatv),
            "_emscripten_glGetFragDataIndex" => func!(_glGetFragDataIndex),
            "_glGetFragDataIndex" => func!(_glGetFragDataIndex),
            "_emscripten_glGetFragDataLocation" => func!(_glGetFragDataLocation),
            "_glGetFragDataLocation" => func!(_glGetFragDataLocation),
            "_emscripten_glGetFramebufferAttachmentParameteriv" => func!(_glGetFramebufferAttachmentParameteriv),
            "_glGetFramebufferAttachmentParameteriv" => func!(_glGetFramebufferAttachmentParameteriv),
            "_emscripten_glGetFramebufferParameteriv" => func!(_glGetFramebufferParameteriv),
            "_glGetFramebufferParameteriv" => func!(_glGetFramebufferParameteriv),
            "_emscripten_glGetGraphicsResetStatus" => func!(_glGetGraphicsResetStatus),
            "_glGetGraphicsResetStatus" => func!(_glGetGraphicsResetStatus),
            "_emscripten_glGetInteger64i_v" => func!(_glGetInteger64i_v),
            "_glGetInteger64i_v" => func!(_glGetInteger64i_v),
            "_emscripten_glGetInteger64v" => func!(_glGetInteger64v),
            "_glGetInteger64v" => func!(_glGetInteger64v),
            "_emscripten_glGetIntegeri_v" => func!(_glGetIntegeri_v),
            "_glGetIntegeri_v" => func!(_glGetIntegeri_v),
            "_emscripten_glGetIntegerv" => func!(_glGetIntegerv),
            "_glGetIntegerv" => func!(_glGetIntegerv),
            "_emscripten_glGetInternalformati64v" => func!(_glGetInternalformati64v),
            "_glGetInternalformati64v" => func!(_glGetInternalformati64v),
            "_emscripten_glGetInternalformativ" => func!(_glGetInternalformativ),
            "_glGetInternalformativ" => func!(_glGetInternalformativ),
            "_emscripten_glGetMultisamplefv" => func!(_glGetMultisamplefv),
            "_glGetMultisamplefv" => func!(_glGetMultisamplefv),
            "_emscripten_glGetNamedBufferParameteri64v" => func!(_glGetNamedBufferParameteri64v),
            "_glGetNamedBufferParameteri64v" => func!(_glGetNamedBufferParameteri64v),
            "_emscripten_glGetNamedBufferParameteriv" => func!(_glGetNamedBufferParameteriv),
            "_glGetNamedBufferParameteriv" => func!(_glGetNamedBufferParameteriv),
            "_emscripten_glGetNamedBufferPointerv" => func!(_glGetNamedBufferPointerv),
            "_glGetNamedBufferPointerv" => func!(_glGetNamedBufferPointerv),
            "_emscripten_glGetNamedBufferSubData" => func!(_glGetNamedBufferSubData),
            "_glGetNamedBufferSubData" => func!(_glGetNamedBufferSubData),
            "_emscripten_glGetNamedFramebufferAttachmentParameteriv" => func!(_glGetNamedFramebufferAttachmentParameteriv),
            "_glGetNamedFramebufferAttachmentParameteriv" => func!(_glGetNamedFramebufferAttachmentParameteriv),
            "_emscripten_glGetNamedFramebufferParameteriv" => func!(_glGetNamedFramebufferParameteriv),
            "_glGetNamedFramebufferParameteriv" => func!(_glGetNamedFramebufferParameteriv),
            "_emscripten_glGetNamedRenderbufferParameteriv" => func!(_glGetNamedRenderbufferParameteriv),
            "_glGetNamedRenderbufferParameteriv" => func!(_glGetNamedRenderbufferParameteriv),
            "_emscripten_glGetObjectLabel" => func!(_glGetObjectLabel),
            "_glGetObjectLabel" => func!(_glGetObjectLabel),
            "_emscripten_glGetObjectPtrLabel" => func!(_glGetObjectPtrLabel),
            "_glGetObjectPtrLabel" => func!(_glGetObjectPtrLabel),
            "_emscripten_glGetPointerv" => func!(_glGetPointerv),
            "_glGetPointerv" => func!(_glGetPointerv),
            "_emscripten_glGetProgramBinary" => func!(_glGetProgramBinary),
            "_glGetProgramBinary" => func!(_glGetProgramBinary),
            "_emscripten_glGetProgramInfoLog" => func!(_glGetProgramInfoLog),
            "_glGetProgramInfoLog" => func!(_glGetProgramInfoLog),
            "_emscripten_glGetProgramInterfaceiv" => func!(_glGetProgramInterfaceiv),
            "_glGetProgramInterfaceiv" => func!(_glGetProgramInterfaceiv),
            "_emscripten_glGetProgramPipelineInfoLog" => func!(_glGetProgramPipelineInfoLog),
            "_glGetProgramPipelineInfoLog" => func!(_glGetProgramPipelineInfoLog),
            "_emscripten_glGetProgramPipelineiv" => func!(_glGetProgramPipelineiv),
            "_glGetProgramPipelineiv" => func!(_glGetProgramPipelineiv),
            "_emscripten_glGetProgramResourceIndex" => func!(_glGetProgramResourceIndex),
            "_glGetProgramResourceIndex" => func!(_glGetProgramResourceIndex),
            "_emscripten_glGetProgramResourceLocation" => func!(_glGetProgramResourceLocation),
            "_glGetProgramResourceLocation" => func!(_glGetProgramResourceLocation),
            "_emscripten_glGetProgramResourceLocationIndex" => func!(_glGetProgramResourceLocationIndex),
            "_glGetProgramResourceLocationIndex" => func!(_glGetProgramResourceLocationIndex),
            "_emscripten_glGetProgramResourceName" => func!(_glGetProgramResourceName),
            "_glGetProgramResourceName" => func!(_glGetProgramResourceName),
            "_emscripten_glGetProgramResourceiv" => func!(_glGetProgramResourceiv),
            "_glGetProgramResourceiv" => func!(_glGetProgramResourceiv),
            "_emscripten_glGetProgramStageiv" => func!(_glGetProgramStageiv),
            "_glGetProgramStageiv" => func!(_glGetProgramStageiv),
            "_emscripten_glGetProgramiv" => func!(_glGetProgramiv),
            "_glGetProgramiv" => func!(_glGetProgramiv),
            "_emscripten_glGetQueryBufferObjecti64v" => func!(_glGetQueryBufferObjecti64v),
            "_glGetQueryBufferObjecti64v" => func!(_glGetQueryBufferObjecti64v),
            "_emscripten_glGetQueryBufferObjectiv" => func!(_glGetQueryBufferObjectiv),
            "_glGetQueryBufferObjectiv" => func!(_glGetQueryBufferObjectiv),
            "_emscripten_glGetQueryBufferObjectui64v" => func!(_glGetQueryBufferObjectui64v),
            "_glGetQueryBufferObjectui64v" => func!(_glGetQueryBufferObjectui64v),
            "_emscripten_glGetQueryBufferObjectuiv" => func!(_glGetQueryBufferObjectuiv),
            "_glGetQueryBufferObjectuiv" => func!(_glGetQueryBufferObjectuiv),
            "_emscripten_glGetQueryIndexediv" => func!(_glGetQueryIndexediv),
            "_glGetQueryIndexediv" => func!(_glGetQueryIndexediv),
            "_emscripten_glGetQueryObjecti64v" => func!(_glGetQueryObjecti64v),
            "_glGetQueryObjecti64v" => func!(_glGetQueryObjecti64v),
            "_emscripten_glGetQueryObjectiv" => func!(_glGetQueryObjectiv),
            "_glGetQueryObjectiv" => func!(_glGetQueryObjectiv),
            "_emscripten_glGetQueryObjectui64v" => func!(_glGetQueryObjectui64v),
            "_glGetQueryObjectui64v" => func!(_glGetQueryObjectui64v),
            "_emscripten_glGetQueryObjectuiv" => func!(_glGetQueryObjectuiv),
            "_glGetQueryObjectuiv" => func!(_glGetQueryObjectuiv),
            "_emscripten_glGetQueryiv" => func!(_glGetQueryiv),
            "_glGetQueryiv" => func!(_glGetQueryiv),
            "_emscripten_glGetRenderbufferParameteriv" => func!(_glGetRenderbufferParameteriv),
            "_glGetRenderbufferParameteriv" => func!(_glGetRenderbufferParameteriv),
            "_emscripten_glGetSamplerParameterIiv" => func!(_glGetSamplerParameterIiv),
            "_glGetSamplerParameterIiv" => func!(_glGetSamplerParameterIiv),
            "_emscripten_glGetSamplerParameterIuiv" => func!(_glGetSamplerParameterIuiv),
            "_glGetSamplerParameterIuiv" => func!(_glGetSamplerParameterIuiv),
            "_emscripten_glGetSamplerParameterfv" => func!(_glGetSamplerParameterfv),
            "_glGetSamplerParameterfv" => func!(_glGetSamplerParameterfv),
            "_emscripten_glGetSamplerParameteriv" => func!(_glGetSamplerParameteriv),
            "_glGetSamplerParameteriv" => func!(_glGetSamplerParameteriv),
            "_emscripten_glGetShaderInfoLog" => func!(_glGetShaderInfoLog),
            "_glGetShaderInfoLog" => func!(_glGetShaderInfoLog),
            "_emscripten_glGetShaderPrecisionFormat" => func!(_glGetShaderPrecisionFormat),
            "_glGetShaderPrecisionFormat" => func!(_glGetShaderPrecisionFormat),
            "_emscripten_glGetShaderSource" => func!(_glGetShaderSource),
            "_glGetShaderSource" => func!(_glGetShaderSource),
            "_emscripten_glGetShaderiv" => func!(_glGetShaderiv),
            "_glGetShaderiv" => func!(_glGetShaderiv),
            "_emscripten_glGetString" => func!(_glGetString),
            "_glGetString" => func!(_glGetString),
            "_emscripten_glGetStringi" => func!(_glGetStringi),
            "_glGetStringi" => func!(_glGetStringi),
            "_emscripten_glGetSubroutineIndex" => func!(_glGetSubroutineIndex),
            "_glGetSubroutineIndex" => func!(_glGetSubroutineIndex),
            "_emscripten_glGetSubroutineUniformLocation" => func!(_glGetSubroutineUniformLocation),
            "_glGetSubroutineUniformLocation" => func!(_glGetSubroutineUniformLocation),
            "_emscripten_glGetSynciv" => func!(_glGetSynciv),
            "_glGetSynciv" => func!(_glGetSynciv),
            "_emscripten_glGetTexImage" => func!(_glGetTexImage),
            "_glGetTexImage" => func!(_glGetTexImage),
            "_emscripten_glGetTexLevelParameterfv" => func!(_glGetTexLevelParameterfv),
            "_glGetTexLevelParameterfv" => func!(_glGetTexLevelParameterfv),
            "_emscripten_glGetTexLevelParameteriv" => func!(_glGetTexLevelParameteriv),
            "_glGetTexLevelParameteriv" => func!(_glGetTexLevelParameteriv),
            "_emscripten_glGetTexParameterIiv" => func!(_glGetTexParameterIiv),
            "_glGetTexParameterIiv" => func!(_glGetTexParameterIiv),
            "_emscripten_glGetTexParameterIuiv" => func!(_glGetTexParameterIuiv),
            "_glGetTexParameterIuiv" => func!(_glGetTexParameterIuiv),
            "_emscripten_glGetTexParameterfv" => func!(_glGetTexParameterfv),
            "_glGetTexParameterfv" => func!(_glGetTexParameterfv),
            "_emscripten_glGetTexParameteriv" => func!(_glGetTexParameteriv),
            "_glGetTexParameteriv" => func!(_glGetTexParameteriv),
            "_emscripten_glGetTextureImage" => func!(_glGetTextureImage),
            "_glGetTextureImage" => func!(_glGetTextureImage),
            "_emscripten_glGetTextureLevelParameterfv" => func!(_glGetTextureLevelParameterfv),
            "_glGetTextureLevelParameterfv" => func!(_glGetTextureLevelParameterfv),
            "_emscripten_glGetTextureLevelParameteriv" => func!(_glGetTextureLevelParameteriv),
            "_glGetTextureLevelParameteriv" => func!(_glGetTextureLevelParameteriv),
            "_emscripten_glGetTextureParameterIiv" => func!(_glGetTextureParameterIiv),
            "_glGetTextureParameterIiv" => func!(_glGetTextureParameterIiv),
            "_emscripten_glGetTextureParameterIuiv" => func!(_glGetTextureParameterIuiv),
            "_glGetTextureParameterIuiv" => func!(_glGetTextureParameterIuiv),
            "_emscripten_glGetTextureParameterfv" => func!(_glGetTextureParameterfv),
            "_glGetTextureParameterfv" => func!(_glGetTextureParameterfv),
            "_emscripten_glGetTextureParameteriv" => func!(_glGetTextureParameteriv),
            "_glGetTextureParameteriv" => func!(_glGetTextureParameteriv),
            "_emscripten_glGetTextureSubImage" => func!(_glGetTextureSubImage),
            "_glGetTextureSubImage" => func!(_glGetTextureSubImage),
            "_emscripten_glGetTransformFeedbackVarying" => func!(_glGetTransformFeedbackVarying),
            "_glGetTransformFeedbackVarying" => func!(_glGetTransformFeedbackVarying),
            "_emscripten_glGetTransformFeedbacki64_v" => func!(_glGetTransformFeedbacki64_v),
            "_glGetTransformFeedbacki64_v" => func!(_glGetTransformFeedbacki64_v),
            "_emscripten_glGetTransformFeedbacki_v" => func!(_glGetTransformFeedbacki_v),
            "_glGetTransformFeedbacki_v" => func!(_glGetTransformFeedbacki_v),
            "_emscripten_glGetTransformFeedbackiv" => func!(_glGetTransformFeedbackiv),
            "_glGetTransformFeedbackiv" => func!(_glGetTransformFeedbackiv),
            "_emscripten_glGetUniformBlockIndex" => func!(_glGetUniformBlockIndex),
            "_glGetUniformBlockIndex" => func!(_glGetUniformBlockIndex),
            "_emscripten_glGetUniformIndices" => func!(_glGetUniformIndices),
            "_glGetUniformIndices" => func!(_glGetUniformIndices),
            "_emscripten_glGetUniformLocation" => func!(_glGetUniformLocation),
            "_glGetUniformLocation" => func!(_glGetUniformLocation),
            "_emscripten_glGetUniformSubroutineuiv" => func!(_glGetUniformSubroutineuiv),
            "_glGetUniformSubroutineuiv" => func!(_glGetUniformSubroutineuiv),
            "_emscripten_glGetUniformdv" => func!(_glGetUniformdv),
            "_glGetUniformdv" => func!(_glGetUniformdv),
            "_emscripten_glGetUniformfv" => func!(_glGetUniformfv),
            "_glGetUniformfv" => func!(_glGetUniformfv),
            "_emscripten_glGetUniformiv" => func!(_glGetUniformiv),
            "_glGetUniformiv" => func!(_glGetUniformiv),
            "_emscripten_glGetUniformuiv" => func!(_glGetUniformuiv),
            "_glGetUniformuiv" => func!(_glGetUniformuiv),
            "_emscripten_glGetVertexArrayIndexed64iv" => func!(_glGetVertexArrayIndexed64iv),
            "_glGetVertexArrayIndexed64iv" => func!(_glGetVertexArrayIndexed64iv),
            "_emscripten_glGetVertexArrayIndexediv" => func!(_glGetVertexArrayIndexediv),
            "_glGetVertexArrayIndexediv" => func!(_glGetVertexArrayIndexediv),
            "_emscripten_glGetVertexArrayiv" => func!(_glGetVertexArrayiv),
            "_glGetVertexArrayiv" => func!(_glGetVertexArrayiv),
            "_emscripten_glGetVertexAttribIiv" => func!(_glGetVertexAttribIiv),
            "_glGetVertexAttribIiv" => func!(_glGetVertexAttribIiv),
            "_emscripten_glGetVertexAttribIuiv" => func!(_glGetVertexAttribIuiv),
            "_glGetVertexAttribIuiv" => func!(_glGetVertexAttribIuiv),
            "_emscripten_glGetVertexAttribLdv" => func!(_glGetVertexAttribLdv),
            "_glGetVertexAttribLdv" => func!(_glGetVertexAttribLdv),
            "_emscripten_glGetVertexAttribPointerv" => func!(_glGetVertexAttribPointerv),
            "_glGetVertexAttribPointerv" => func!(_glGetVertexAttribPointerv),
            "_emscripten_glGetVertexAttribdv" => func!(_glGetVertexAttribdv),
            "_glGetVertexAttribdv" => func!(_glGetVertexAttribdv),
            "_emscripten_glGetVertexAttribfv" => func!(_glGetVertexAttribfv),
            "_glGetVertexAttribfv" => func!(_glGetVertexAttribfv),
            "_emscripten_glGetVertexAttribiv" => func!(_glGetVertexAttribiv),
            "_glGetVertexAttribiv" => func!(_glGetVertexAttribiv),
            "_emscripten_glGetnColorTable" => func!(_glGetnColorTable),
            "_glGetnColorTable" => func!(_glGetnColorTable),
            "_emscripten_glGetnCompressedTexImage" => func!(_glGetnCompressedTexImage),
            "_glGetnCompressedTexImage" => func!(_glGetnCompressedTexImage),
            "_emscripten_glGetnConvolutionFilter" => func!(_glGetnConvolutionFilter),
            "_glGetnConvolutionFilter" => func!(_glGetnConvolutionFilter),
            "_emscripten_glGetnHistogram" => func!(_glGetnHistogram),
            "_glGetnHistogram" => func!(_glGetnHistogram),
            "_emscripten_glGetnMapdv" => func!(_glGetnMapdv),
            "_glGetnMapdv" => func!(_glGetnMapdv),
            "_emscripten_glGetnMapfv" => func!(_glGetnMapfv),
            "_glGetnMapfv" => func!(_glGetnMapfv),
            "_emscripten_glGetnMapiv" => func!(_glGetnMapiv),
            "_glGetnMapiv" => func!(_glGetnMapiv),
            "_emscripten_glGetnMinmax" => func!(_glGetnMinmax),
            "_glGetnMinmax" => func!(_glGetnMinmax),
            "_emscripten_glGetnPixelMapfv" => func!(_glGetnPixelMapfv),
            "_glGetnPixelMapfv" => func!(_glGetnPixelMapfv),
            "_emscripten_glGetnPixelMapuiv" => func!(_glGetnPixelMapuiv),
            "_glGetnPixelMapuiv" => func!(_glGetnPixelMapuiv),
            "_emscripten_glGetnPixelMapusv" => func!(_glGetnPixelMapusv),
            "_glGetnPixelMapusv" => func!(_glGetnPixelMapusv),
            "_emscripten_glGetnPolygonStipple" => func!(_glGetnPolygonStipple),
            "_glGetnPolygonStipple" => func!(_glGetnPolygonStipple),
            "_emscripten_glGetnSeparableFilter" => func!(_glGetnSeparableFilter),
            "_glGetnSeparableFilter" => func!(_glGetnSeparableFilter),
            "_emscripten_glGetnTexImage" => func!(_glGetnTexImage),
            "_glGetnTexImage" => func!(_glGetnTexImage),
            "_emscripten_glGetnUniformdv" => func!(_glGetnUniformdv),
            "_glGetnUniformdv" => func!(_glGetnUniformdv),
            "_emscripten_glGetnUniformfv" => func!(_glGetnUniformfv),
            "_glGetnUniformfv" => func!(_glGetnUniformfv),
            "_emscripten_glGetnUniformiv" => func!(_glGetnUniformiv),
            "_glGetnUniformiv" => func!(_glGetnUniformiv),
            "_emscripten_glGetnUniformuiv" => func!(_glGetnUniformuiv),
            "_glGetnUniformuiv" => func!(_glGetnUniformuiv),
            "_emscripten_glHint" => func!(_glHint),
            "_glHint" => func!(_glHint),
            "_emscripten_glInvalidateBufferData" => func!(_glInvalidateBufferData),
            "_glInvalidateBufferData" => func!(_glInvalidateBufferData),
            "_emscripten_glInvalidateBufferSubData" => func!(_glInvalidateBufferSubData),
            "_glInvalidateBufferSubData" => func!(_glInvalidateBufferSubData),
            "_emscripten_glInvalidateFramebuffer" => func!(_glInvalidateFramebuffer),
            "_glInvalidateFramebuffer" => func!(_glInvalidateFramebuffer),
            "_emscripten_glInvalidateNamedFramebufferData" => func!(_glInvalidateNamedFramebufferData),
            "_glInvalidateNamedFramebufferData" => func!(_glInvalidateNamedFramebufferData),
            "_emscripten_glInvalidateNamedFramebufferSubData" => func!(_glInvalidateNamedFramebufferSubData),
            "_glInvalidateNamedFramebufferSubData" => func!(_glInvalidateNamedFramebufferSubData),
            "_emscripten_glInvalidateSubFramebuffer" => func!(_glInvalidateSubFramebuffer),
            "_glInvalidateSubFramebuffer" => func!(_glInvalidateSubFramebuffer),
            "_emscripten_glInvalidateTexImage" => func!(_glInvalidateTexImage),
            "_glInvalidateTexImage" => func!(_glInvalidateTexImage),
            "_emscripten_glInvalidateTexSubImage" => func!(_glInvalidateTexSubImage),
            "_glInvalidateTexSubImage" => func!(_glInvalidateTexSubImage),
            "_emscripten_glIsBuffer" => func!(_glIsBuffer),
            "_glIsBuffer" => func!(_glIsBuffer),
            "_emscripten_glIsEnabled" => func!(_glIsEnabled),
            "_glIsEnabled" => func!(_glIsEnabled),
            "_emscripten_glIsEnabledi" => func!(_glIsEnabledi),
            "_glIsEnabledi" => func!(_glIsEnabledi),
            "_emscripten_glIsFramebuffer" => func!(_glIsFramebuffer),
            "_glIsFramebuffer" => func!(_glIsFramebuffer),
            "_emscripten_glIsProgram" => func!(_glIsProgram),
            "_glIsProgram" => func!(_glIsProgram),
            "_emscripten_glIsProgramPipeline" => func!(_glIsProgramPipeline),
            "_glIsProgramPipeline" => func!(_glIsProgramPipeline),
            "_emscripten_glIsQuery" => func!(_glIsQuery),
            "_glIsQuery" => func!(_glIsQuery),
            "_emscripten_glIsRenderbuffer" => func!(_glIsRenderbuffer),
            "_glIsRenderbuffer" => func!(_glIsRenderbuffer),
            "_emscripten_glIsSampler" => func!(_glIsSampler),
            "_glIsSampler" => func!(_glIsSampler),
            "_emscripten_glIsShader" => func!(_glIsShader),
            "_glIsShader" => func!(_glIsShader),
            "_emscripten_glIsSync" => func!(_glIsSync),
            "_glIsSync" => func!(_glIsSync),
            "_emscripten_glIsTexture" => func!(_glIsTexture),
            "_glIsTexture" => func!(_glIsTexture),
            "_emscripten_glIsTransformFeedback" => func!(_glIsTransformFeedback),
            "_glIsTransformFeedback" => func!(_glIsTransformFeedback),
            "_emscripten_glIsVertexArray" => func!(_glIsVertexArray),
            "_glIsVertexArray" => func!(_glIsVertexArray),
            "_emscripten_glLineWidth" => func!(_glLineWidth),
            "_glLineWidth" => func!(_glLineWidth),
            "_emscripten_glLinkProgram" => func!(_glLinkProgram),
            "_glLinkProgram" => func!(_glLinkProgram),
            "_emscripten_glLogicOp" => func!(_glLogicOp),
            "_glLogicOp" => func!(_glLogicOp),
            "_emscripten_glMapBuffer" => func!(_glMapBuffer),
            "_glMapBuffer" => func!(_glMapBuffer),
            "_emscripten_glMapBufferRange" => func!(_glMapBufferRange),
            "_glMapBufferRange" => func!(_glMapBufferRange),
            "_emscripten_glMapNamedBuffer" => func!(_glMapNamedBuffer),
            "_glMapNamedBuffer" => func!(_glMapNamedBuffer),
            "_emscripten_glMapNamedBufferRange" => func!(_glMapNamedBufferRange),
            "_glMapNamedBufferRange" => func!(_glMapNamedBufferRange),
            "_emscripten_glMemoryBarrier" => func!(_glMemoryBarrier),
            "_glMemoryBarrier" => func!(_glMemoryBarrier),
            "_emscripten_glMemoryBarrierByRegion" => func!(_glMemoryBarrierByRegion),
            "_glMemoryBarrierByRegion" => func!(_glMemoryBarrierByRegion),
            "_emscripten_glMinSampleShading" => func!(_glMinSampleShading),
            "_glMinSampleShading" => func!(_glMinSampleShading),
            "_emscripten_glMultiDrawArrays" => func!(_glMultiDrawArrays),
            "_glMultiDrawArrays" => func!(_glMultiDrawArrays),
            "_emscripten_glMultiDrawArraysIndirect" => func!(_glMultiDrawArraysIndirect),
            "_glMultiDrawArraysIndirect" => func!(_glMultiDrawArraysIndirect),
            "_emscripten_glMultiDrawElements" => func!(_glMultiDrawElements),
            "_glMultiDrawElements" => func!(_glMultiDrawElements),
            "_emscripten_glMultiDrawElementsBaseVertex" => func!(_glMultiDrawElementsBaseVertex),
            "_glMultiDrawElementsBaseVertex" => func!(_glMultiDrawElementsBaseVertex),
            "_emscripten_glMultiDrawElementsIndirect" => func!(_glMultiDrawElementsIndirect),
            "_glMultiDrawElementsIndirect" => func!(_glMultiDrawElementsIndirect),
            "_emscripten_glMultiTexCoordP1ui" => func!(_glMultiTexCoordP1ui),
            "_glMultiTexCoordP1ui" => func!(_glMultiTexCoordP1ui),
            "_emscripten_glMultiTexCoordP1uiv" => func!(_glMultiTexCoordP1uiv),
            "_glMultiTexCoordP1uiv" => func!(_glMultiTexCoordP1uiv),
            "_emscripten_glMultiTexCoordP2ui" => func!(_glMultiTexCoordP2ui),
            "_glMultiTexCoordP2ui" => func!(_glMultiTexCoordP2ui),
            "_emscripten_glMultiTexCoordP2uiv" => func!(_glMultiTexCoordP2uiv),
            "_glMultiTexCoordP2uiv" => func!(_glMultiTexCoordP2uiv),
            "_emscripten_glMultiTexCoordP3ui" => func!(_glMultiTexCoordP3ui),
            "_glMultiTexCoordP3ui" => func!(_glMultiTexCoordP3ui),
            "_emscripten_glMultiTexCoordP3uiv" => func!(_glMultiTexCoordP3uiv),
            "_glMultiTexCoordP3uiv" => func!(_glMultiTexCoordP3uiv),
            "_emscripten_glMultiTexCoordP4ui" => func!(_glMultiTexCoordP4ui),
            "_glMultiTexCoordP4ui" => func!(_glMultiTexCoordP4ui),
            "_emscripten_glMultiTexCoordP4uiv" => func!(_glMultiTexCoordP4uiv),
            "_glMultiTexCoordP4uiv" => func!(_glMultiTexCoordP4uiv),
            "_emscripten_glNamedBufferData" => func!(_glNamedBufferData),
            "_glNamedBufferData" => func!(_glNamedBufferData),
            "_emscripten_glNamedBufferStorage" => func!(_glNamedBufferStorage),
            "_glNamedBufferStorage" => func!(_glNamedBufferStorage),
            "_emscripten_glNamedBufferSubData" => func!(_glNamedBufferSubData),
            "_glNamedBufferSubData" => func!(_glNamedBufferSubData),
            "_emscripten_glNamedFramebufferDrawBuffer" => func!(_glNamedFramebufferDrawBuffer),
            "_glNamedFramebufferDrawBuffer" => func!(_glNamedFramebufferDrawBuffer),
            "_emscripten_glNamedFramebufferDrawBuffers" => func!(_glNamedFramebufferDrawBuffers),
            "_glNamedFramebufferDrawBuffers" => func!(_glNamedFramebufferDrawBuffers),
            "_emscripten_glNamedFramebufferParameteri" => func!(_glNamedFramebufferParameteri),
            "_glNamedFramebufferParameteri" => func!(_glNamedFramebufferParameteri),
            "_emscripten_glNamedFramebufferReadBuffer" => func!(_glNamedFramebufferReadBuffer),
            "_glNamedFramebufferReadBuffer" => func!(_glNamedFramebufferReadBuffer),
            "_emscripten_glNamedFramebufferRenderbuffer" => func!(_glNamedFramebufferRenderbuffer),
            "_glNamedFramebufferRenderbuffer" => func!(_glNamedFramebufferRenderbuffer),
            "_emscripten_glNamedFramebufferTexture" => func!(_glNamedFramebufferTexture),
            "_glNamedFramebufferTexture" => func!(_glNamedFramebufferTexture),
            "_emscripten_glNamedFramebufferTextureLayer" => func!(_glNamedFramebufferTextureLayer),
            "_glNamedFramebufferTextureLayer" => func!(_glNamedFramebufferTextureLayer),
            "_emscripten_glNamedRenderbufferStorage" => func!(_glNamedRenderbufferStorage),
            "_glNamedRenderbufferStorage" => func!(_glNamedRenderbufferStorage),
            "_emscripten_glNamedRenderbufferStorageMultisample" => func!(_glNamedRenderbufferStorageMultisample),
            "_glNamedRenderbufferStorageMultisample" => func!(_glNamedRenderbufferStorageMultisample),
            "_emscripten_glNormalP3ui" => func!(_glNormalP3ui),
            "_glNormalP3ui" => func!(_glNormalP3ui),
            "_emscripten_glNormalP3uiv" => func!(_glNormalP3uiv),
            "_glNormalP3uiv" => func!(_glNormalP3uiv),
            "_emscripten_glObjectLabel" => func!(_glObjectLabel),
            "_glObjectLabel" => func!(_glObjectLabel),
            "_emscripten_glObjectPtrLabel" => func!(_glObjectPtrLabel),
            "_glObjectPtrLabel" => func!(_glObjectPtrLabel),
            "_emscripten_glPatchParameterfv" => func!(_glPatchParameterfv),
            "_glPatchParameterfv" => func!(_glPatchParameterfv),
            "_emscripten_glPatchParameteri" => func!(_glPatchParameteri),
            "_glPatchParameteri" => func!(_glPatchParameteri),
            "_emscripten_glPauseTransformFeedback" => func!(_glPauseTransformFeedback),
            "_glPauseTransformFeedback" => func!(_glPauseTransformFeedback),
            "_emscripten_glPixelStoref" => func!(_glPixelStoref),
            "_glPixelStoref" => func!(_glPixelStoref),
            "_emscripten_glPixelStorei" => func!(_glPixelStorei),
            "_glPixelStorei" => func!(_glPixelStorei),
            "_emscripten_glPointParameterf" => func!(_glPointParameterf),
            "_glPointParameterf" => func!(_glPointParameterf),
            "_emscripten_glPointParameterfv" => func!(_glPointParameterfv),
            "_glPointParameterfv" => func!(_glPointParameterfv),
            "_emscripten_glPointParameteri" => func!(_glPointParameteri),
            "_glPointParameteri" => func!(_glPointParameteri),
            "_emscripten_glPointParameteriv" => func!(_glPointParameteriv),
            "_glPointParameteriv" => func!(_glPointParameteriv),
            "_emscripten_glPointSize" => func!(_glPointSize),
            "_glPointSize" => func!(_glPointSize),
            "_emscripten_glPolygonMode" => func!(_glPolygonMode),
            "_glPolygonMode" => func!(_glPolygonMode),
            "_emscripten_glPolygonOffset" => func!(_glPolygonOffset),
            "_glPolygonOffset" => func!(_glPolygonOffset),
            "_emscripten_glPopDebugGroup" => func!(_glPopDebugGroup),
            "_glPopDebugGroup" => func!(_glPopDebugGroup),
            "_emscripten_glPrimitiveRestartIndex" => func!(_glPrimitiveRestartIndex),
            "_glPrimitiveRestartIndex" => func!(_glPrimitiveRestartIndex),
            "_emscripten_glProgramBinary" => func!(_glProgramBinary),
            "_glProgramBinary" => func!(_glProgramBinary),
            "_emscripten_glProgramParameteri" => func!(_glProgramParameteri),
            "_glProgramParameteri" => func!(_glProgramParameteri),
            "_emscripten_glProgramUniform1d" => func!(_glProgramUniform1d),
            "_glProgramUniform1d" => func!(_glProgramUniform1d),
            "_emscripten_glProgramUniform1dv" => func!(_glProgramUniform1dv),
            "_glProgramUniform1dv" => func!(_glProgramUniform1dv),
            "_emscripten_glProgramUniform1f" => func!(_glProgramUniform1f),
            "_glProgramUniform1f" => func!(_glProgramUniform1f),
            "_emscripten_glProgramUniform1fv" => func!(_glProgramUniform1fv),
            "_glProgramUniform1fv" => func!(_glProgramUniform1fv),
            "_emscripten_glProgramUniform1i" => func!(_glProgramUniform1i),
            "_glProgramUniform1i" => func!(_glProgramUniform1i),
            "_emscripten_glProgramUniform1iv" => func!(_glProgramUniform1iv),
            "_glProgramUniform1iv" => func!(_glProgramUniform1iv),
            "_emscripten_glProgramUniform1ui" => func!(_glProgramUniform1ui),
            "_glProgramUniform1ui" => func!(_glProgramUniform1ui),
            "_emscripten_glProgramUniform1uiv" => func!(_glProgramUniform1uiv),
            "_glProgramUniform1uiv" => func!(_glProgramUniform1uiv),
            "_emscripten_glProgramUniform2d" => func!(_glProgramUniform2d),
            "_glProgramUniform2d" => func!(_glProgramUniform2d),
            "_emscripten_glProgramUniform2dv" => func!(_glProgramUniform2dv),
            "_glProgramUniform2dv" => func!(_glProgramUniform2dv),
            "_emscripten_glProgramUniform2f" => func!(_glProgramUniform2f),
            "_glProgramUniform2f" => func!(_glProgramUniform2f),
            "_emscripten_glProgramUniform2fv" => func!(_glProgramUniform2fv),
            "_glProgramUniform2fv" => func!(_glProgramUniform2fv),
            "_emscripten_glProgramUniform2i" => func!(_glProgramUniform2i),
            "_glProgramUniform2i" => func!(_glProgramUniform2i),
            "_emscripten_glProgramUniform2iv" => func!(_glProgramUniform2iv),
            "_glProgramUniform2iv" => func!(_glProgramUniform2iv),
            "_emscripten_glProgramUniform2ui" => func!(_glProgramUniform2ui),
            "_glProgramUniform2ui" => func!(_glProgramUniform2ui),
            "_emscripten_glProgramUniform2uiv" => func!(_glProgramUniform2uiv),
            "_glProgramUniform2uiv" => func!(_glProgramUniform2uiv),
            "_emscripten_glProgramUniform3d" => func!(_glProgramUniform3d),
            "_glProgramUniform3d" => func!(_glProgramUniform3d),
            "_emscripten_glProgramUniform3dv" => func!(_glProgramUniform3dv),
            "_glProgramUniform3dv" => func!(_glProgramUniform3dv),
            "_emscripten_glProgramUniform3f" => func!(_glProgramUniform3f),
            "_glProgramUniform3f" => func!(_glProgramUniform3f),
            "_emscripten_glProgramUniform3fv" => func!(_glProgramUniform3fv),
            "_glProgramUniform3fv" => func!(_glProgramUniform3fv),
            "_emscripten_glProgramUniform3i" => func!(_glProgramUniform3i),
            "_glProgramUniform3i" => func!(_glProgramUniform3i),
            "_emscripten_glProgramUniform3iv" => func!(_glProgramUniform3iv),
            "_glProgramUniform3iv" => func!(_glProgramUniform3iv),
            "_emscripten_glProgramUniform3ui" => func!(_glProgramUniform3ui),
            "_glProgramUniform3ui" => func!(_glProgramUniform3ui),
            "_emscripten_glProgramUniform3uiv" => func!(_glProgramUniform3uiv),
            "_glProgramUniform3uiv" => func!(_glProgramUniform3uiv),
            "_emscripten_glProgramUniform4d" => func!(_glProgramUniform4d),
            "_glProgramUniform4d" => func!(_glProgramUniform4d),
            "_emscripten_glProgramUniform4dv" => func!(_glProgramUniform4dv),
            "_glProgramUniform4dv" => func!(_glProgramUniform4dv),
            "_emscripten_glProgramUniform4f" => func!(_glProgramUniform4f),
            "_glProgramUniform4f" => func!(_glProgramUniform4f),
            "_emscripten_glProgramUniform4fv" => func!(_glProgramUniform4fv),
            "_glProgramUniform4fv" => func!(_glProgramUniform4fv),
            "_emscripten_glProgramUniform4i" => func!(_glProgramUniform4i),
            "_glProgramUniform4i" => func!(_glProgramUniform4i),
            "_emscripten_glProgramUniform4iv" => func!(_glProgramUniform4iv),
            "_glProgramUniform4iv" => func!(_glProgramUniform4iv),
            "_emscripten_glProgramUniform4ui" => func!(_glProgramUniform4ui),
            "_glProgramUniform4ui" => func!(_glProgramUniform4ui),
            "_emscripten_glProgramUniform4uiv" => func!(_glProgramUniform4uiv),
            "_glProgramUniform4uiv" => func!(_glProgramUniform4uiv),
            "_emscripten_glProgramUniformMatrix2dv" => func!(_glProgramUniformMatrix2dv),
            "_glProgramUniformMatrix2dv" => func!(_glProgramUniformMatrix2dv),
            "_emscripten_glProgramUniformMatrix2fv" => func!(_glProgramUniformMatrix2fv),
            "_glProgramUniformMatrix2fv" => func!(_glProgramUniformMatrix2fv),
            "_emscripten_glProgramUniformMatrix2x3dv" => func!(_glProgramUniformMatrix2x3dv),
            "_glProgramUniformMatrix2x3dv" => func!(_glProgramUniformMatrix2x3dv),
            "_emscripten_glProgramUniformMatrix2x3fv" => func!(_glProgramUniformMatrix2x3fv),
            "_glProgramUniformMatrix2x3fv" => func!(_glProgramUniformMatrix2x3fv),
            "_emscripten_glProgramUniformMatrix2x4dv" => func!(_glProgramUniformMatrix2x4dv),
            "_glProgramUniformMatrix2x4dv" => func!(_glProgramUniformMatrix2x4dv),
            "_emscripten_glProgramUniformMatrix2x4fv" => func!(_glProgramUniformMatrix2x4fv),
            "_glProgramUniformMatrix2x4fv" => func!(_glProgramUniformMatrix2x4fv),
            "_emscripten_glProgramUniformMatrix3dv" => func!(_glProgramUniformMatrix3dv),
            "_glProgramUniformMatrix3dv" => func!(_glProgramUniformMatrix3dv),
            "_emscripten_glProgramUniformMatrix3fv" => func!(_glProgramUniformMatrix3fv),
            "_glProgramUniformMatrix3fv" => func!(_glProgramUniformMatrix3fv),
            "_emscripten_glProgramUniformMatrix3x2dv" => func!(_glProgramUniformMatrix3x2dv),
            "_glProgramUniformMatrix3x2dv" => func!(_glProgramUniformMatrix3x2dv),
            "_emscripten_glProgramUniformMatrix3x2fv" => func!(_glProgramUniformMatrix3x2fv),
            "_glProgramUniformMatrix3x2fv" => func!(_glProgramUniformMatrix3x2fv),
            "_emscripten_glProgramUniformMatrix3x4dv" => func!(_glProgramUniformMatrix3x4dv),
            "_glProgramUniformMatrix3x4dv" => func!(_glProgramUniformMatrix3x4dv),
            "_emscripten_glProgramUniformMatrix3x4fv" => func!(_glProgramUniformMatrix3x4fv),
            "_glProgramUniformMatrix3x4fv" => func!(_glProgramUniformMatrix3x4fv),
            "_emscripten_glProgramUniformMatrix4dv" => func!(_glProgramUniformMatrix4dv),
            "_glProgramUniformMatrix4dv" => func!(_glProgramUniformMatrix4dv),
            "_emscripten_glProgramUniformMatrix4fv" => func!(_glProgramUniformMatrix4fv),
            "_glProgramUniformMatrix4fv" => func!(_glProgramUniformMatrix4fv),
            "_emscripten_glProgramUniformMatrix4x2dv" => func!(_glProgramUniformMatrix4x2dv),
            "_glProgramUniformMatrix4x2dv" => func!(_glProgramUniformMatrix4x2dv),
            "_emscripten_glProgramUniformMatrix4x2fv" => func!(_glProgramUniformMatrix4x2fv),
            "_glProgramUniformMatrix4x2fv" => func!(_glProgramUniformMatrix4x2fv),
            "_emscripten_glProgramUniformMatrix4x3dv" => func!(_glProgramUniformMatrix4x3dv),
            "_glProgramUniformMatrix4x3dv" => func!(_glProgramUniformMatrix4x3dv),
            "_emscripten_glProgramUniformMatrix4x3fv" => func!(_glProgramUniformMatrix4x3fv),
            "_glProgramUniformMatrix4x3fv" => func!(_glProgramUniformMatrix4x3fv),
            "_emscripten_glProvokingVertex" => func!(_glProvokingVertex),
            "_glProvokingVertex" => func!(_glProvokingVertex),
            "_emscripten_glPushDebugGroup" => func!(_glPushDebugGroup),
            "_glPushDebugGroup" => func!(_glPushDebugGroup),
            "_emscripten_glQueryCounter" => func!(_glQueryCounter),
            "_glQueryCounter" => func!(_glQueryCounter),
            "_emscripten_glReadBuffer" => func!(_glReadBuffer),
            "_glReadBuffer" => func!(_glReadBuffer),
            "_emscripten_glReadPixels" => func!(_glReadPixels),
            "_glReadPixels" => func!(_glReadPixels),
            "_emscripten_glReadnPixels" => func!(_glReadnPixels),
            "_glReadnPixels" => func!(_glReadnPixels),
            "_emscripten_glReleaseShaderCompiler" => func!(_glReleaseShaderCompiler),
            "_glReleaseShaderCompiler" => func!(_glReleaseShaderCompiler),
            "_emscripten_glRenderbufferStorage" => func!(_glRenderbufferStorage),
            "_glRenderbufferStorage" => func!(_glRenderbufferStorage),
            "_emscripten_glRenderbufferStorageMultisample" => func!(_glRenderbufferStorageMultisample),
            "_glRenderbufferStorageMultisample" => func!(_glRenderbufferStorageMultisample),
            "_emscripten_glResumeTransformFeedback" => func!(_glResumeTransformFeedback),
            "_glResumeTransformFeedback" => func!(_glResumeTransformFeedback),
            "_emscripten_glSampleCoverage" => func!(_glSampleCoverage),
            "_glSampleCoverage" => func!(_glSampleCoverage),
            "_emscripten_glSampleMaski" => func!(_glSampleMaski),
            "_glSampleMaski" => func!(_glSampleMaski),
            "_emscripten_glSamplerParameterIiv" => func!(_glSamplerParameterIiv),
            "_glSamplerParameterIiv" => func!(_glSamplerParameterIiv),
            "_emscripten_glSamplerParameterIuiv" => func!(_glSamplerParameterIuiv),
            "_glSamplerParameterIuiv" => func!(_glSamplerParameterIuiv),
            "_emscripten_glSamplerParameterf" => func!(_glSamplerParameterf),
            "_glSamplerParameterf" => func!(_glSamplerParameterf),
            "_emscripten_glSamplerParameterfv" => func!(_glSamplerParameterfv),
            "_glSamplerParameterfv" => func!(_glSamplerParameterfv),
            "_emscripten_glSamplerParameteri" => func!(_glSamplerParameteri),
            "_glSamplerParameteri" => func!(_glSamplerParameteri),
            "_emscripten_glSamplerParameteriv" => func!(_glSamplerParameteriv),
            "_glSamplerParameteriv" => func!(_glSamplerParameteriv),
            "_emscripten_glScissor" => func!(_glScissor),
            "_glScissor" => func!(_glScissor),
            "_emscripten_glScissorArrayv" => func!(_glScissorArrayv),
            "_glScissorArrayv" => func!(_glScissorArrayv),
            "_emscripten_glScissorIndexed" => func!(_glScissorIndexed),
            "_glScissorIndexed" => func!(_glScissorIndexed),
            "_emscripten_glScissorIndexedv" => func!(_glScissorIndexedv),
            "_glScissorIndexedv" => func!(_glScissorIndexedv),
            "_emscripten_glSecondaryColorP3ui" => func!(_glSecondaryColorP3ui),
            "_glSecondaryColorP3ui" => func!(_glSecondaryColorP3ui),
            "_emscripten_glSecondaryColorP3uiv" => func!(_glSecondaryColorP3uiv),
            "_glSecondaryColorP3uiv" => func!(_glSecondaryColorP3uiv),
            "_emscripten_glShaderBinary" => func!(_glShaderBinary),
            "_glShaderBinary" => func!(_glShaderBinary),
            "_emscripten_glShaderSource" => func!(_glShaderSource),
            "_glShaderSource" => func!(_glShaderSource),
            "_emscripten_glShaderStorageBlockBinding" => func!(_glShaderStorageBlockBinding),
            "_glShaderStorageBlockBinding" => func!(_glShaderStorageBlockBinding),
            "_emscripten_glStencilFunc" => func!(_glStencilFunc),
            "_glStencilFunc" => func!(_glStencilFunc),
            "_emscripten_glStencilFuncSeparate" => func!(_glStencilFuncSeparate),
            "_glStencilFuncSeparate" => func!(_glStencilFuncSeparate),
            "_emscripten_glStencilMask" => func!(_glStencilMask),
            "_glStencilMask" => func!(_glStencilMask),
            "_emscripten_glStencilMaskSeparate" => func!(_glStencilMaskSeparate),
            "_glStencilMaskSeparate" => func!(_glStencilMaskSeparate),
            "_emscripten_glStencilOp" => func!(_glStencilOp),
            "_glStencilOp" => func!(_glStencilOp),
            "_emscripten_glStencilOpSeparate" => func!(_glStencilOpSeparate),
            "_glStencilOpSeparate" => func!(_glStencilOpSeparate),
            "_emscripten_glTexBuffer" => func!(_glTexBuffer),
            "_glTexBuffer" => func!(_glTexBuffer),
            "_emscripten_glTexBufferRange" => func!(_glTexBufferRange),
            "_glTexBufferRange" => func!(_glTexBufferRange),
            "_emscripten_glTexCoordP1ui" => func!(_glTexCoordP1ui),
            "_glTexCoordP1ui" => func!(_glTexCoordP1ui),
            "_emscripten_glTexCoordP1uiv" => func!(_glTexCoordP1uiv),
            "_glTexCoordP1uiv" => func!(_glTexCoordP1uiv),
            "_emscripten_glTexCoordP2ui" => func!(_glTexCoordP2ui),
            "_glTexCoordP2ui" => func!(_glTexCoordP2ui),
            "_emscripten_glTexCoordP2uiv" => func!(_glTexCoordP2uiv),
            "_glTexCoordP2uiv" => func!(_glTexCoordP2uiv),
            "_emscripten_glTexCoordP3ui" => func!(_glTexCoordP3ui),
            "_glTexCoordP3ui" => func!(_glTexCoordP3ui),
            "_emscripten_glTexCoordP3uiv" => func!(_glTexCoordP3uiv),
            "_glTexCoordP3uiv" => func!(_glTexCoordP3uiv),
            "_emscripten_glTexCoordP4ui" => func!(_glTexCoordP4ui),
            "_glTexCoordP4ui" => func!(_glTexCoordP4ui),
            "_emscripten_glTexCoordP4uiv" => func!(_glTexCoordP4uiv),
            "_glTexCoordP4uiv" => func!(_glTexCoordP4uiv),
            "_emscripten_glTexImage1D" => func!(_glTexImage1D),
            "_glTexImage1D" => func!(_glTexImage1D),
            "_emscripten_glTexImage2D" => func!(_glTexImage2D),
            "_glTexImage2D" => func!(_glTexImage2D),
            "_emscripten_glTexImage2DMultisample" => func!(_glTexImage2DMultisample),
            "_glTexImage2DMultisample" => func!(_glTexImage2DMultisample),
            "_emscripten_glTexImage3D" => func!(_glTexImage3D),
            "_glTexImage3D" => func!(_glTexImage3D),
            "_emscripten_glTexImage3DMultisample" => func!(_glTexImage3DMultisample),
            "_glTexImage3DMultisample" => func!(_glTexImage3DMultisample),
            "_emscripten_glTexParameterIiv" => func!(_glTexParameterIiv),
            "_glTexParameterIiv" => func!(_glTexParameterIiv),
            "_emscripten_glTexParameterIuiv" => func!(_glTexParameterIuiv),
            "_glTexParameterIuiv" => func!(_glTexParameterIuiv),
            "_emscripten_glTexParameterf" => func!(_glTexParameterf),
            "_glTexParameterf" => func!(_glTexParameterf),
            "_emscripten_glTexParameterfv" => func!(_glTexParameterfv),
            "_glTexParameterfv" => func!(_glTexParameterfv),
            "_emscripten_glTexParameteri" => func!(_glTexParameteri),
            "_glTexParameteri" => func!(_glTexParameteri),
            "_emscripten_glTexParameteriv" => func!(_glTexParameteriv),
            "_glTexParameteriv" => func!(_glTexParameteriv),
            "_emscripten_glTexStorage1D" => func!(_glTexStorage1D),
            "_glTexStorage1D" => func!(_glTexStorage1D),
            "_emscripten_glTexStorage2D" => func!(_glTexStorage2D),
            "_glTexStorage2D" => func!(_glTexStorage2D),
            "_emscripten_glTexStorage2DMultisample" => func!(_glTexStorage2DMultisample),
            "_glTexStorage2DMultisample" => func!(_glTexStorage2DMultisample),
            "_emscripten_glTexStorage3D" => func!(_glTexStorage3D),
            "_glTexStorage3D" => func!(_glTexStorage3D),
            "_emscripten_glTexStorage3DMultisample" => func!(_glTexStorage3DMultisample),
            "_glTexStorage3DMultisample" => func!(_glTexStorage3DMultisample),
            "_emscripten_glTexSubImage1D" => func!(_glTexSubImage1D),
            "_glTexSubImage1D" => func!(_glTexSubImage1D),
            "_emscripten_glTexSubImage2D" => func!(_glTexSubImage2D),
            "_glTexSubImage2D" => func!(_glTexSubImage2D),
            "_emscripten_glTexSubImage3D" => func!(_glTexSubImage3D),
            "_glTexSubImage3D" => func!(_glTexSubImage3D),
            "_emscripten_glTextureBarrier" => func!(_glTextureBarrier),
            "_glTextureBarrier" => func!(_glTextureBarrier),
            "_emscripten_glTextureBuffer" => func!(_glTextureBuffer),
            "_glTextureBuffer" => func!(_glTextureBuffer),
            "_emscripten_glTextureBufferRange" => func!(_glTextureBufferRange),
            "_glTextureBufferRange" => func!(_glTextureBufferRange),
            "_emscripten_glTextureParameterIiv" => func!(_glTextureParameterIiv),
            "_glTextureParameterIiv" => func!(_glTextureParameterIiv),
            "_emscripten_glTextureParameterIuiv" => func!(_glTextureParameterIuiv),
            "_glTextureParameterIuiv" => func!(_glTextureParameterIuiv),
            "_emscripten_glTextureParameterf" => func!(_glTextureParameterf),
            "_glTextureParameterf" => func!(_glTextureParameterf),
            "_emscripten_glTextureParameterfv" => func!(_glTextureParameterfv),
            "_glTextureParameterfv" => func!(_glTextureParameterfv),
            "_emscripten_glTextureParameteri" => func!(_glTextureParameteri),
            "_glTextureParameteri" => func!(_glTextureParameteri),
            "_emscripten_glTextureParameteriv" => func!(_glTextureParameteriv),
            "_glTextureParameteriv" => func!(_glTextureParameteriv),
            "_emscripten_glTextureStorage1D" => func!(_glTextureStorage1D),
            "_glTextureStorage1D" => func!(_glTextureStorage1D),
            "_emscripten_glTextureStorage2D" => func!(_glTextureStorage2D),
            "_glTextureStorage2D" => func!(_glTextureStorage2D),
            "_emscripten_glTextureStorage2DMultisample" => func!(_glTextureStorage2DMultisample),
            "_glTextureStorage2DMultisample" => func!(_glTextureStorage2DMultisample),
            "_emscripten_glTextureStorage3D" => func!(_glTextureStorage3D),
            "_glTextureStorage3D" => func!(_glTextureStorage3D),
            "_emscripten_glTextureStorage3DMultisample" => func!(_glTextureStorage3DMultisample),
            "_glTextureStorage3DMultisample" => func!(_glTextureStorage3DMultisample),
            "_emscripten_glTextureSubImage1D" => func!(_glTextureSubImage1D),
            "_glTextureSubImage1D" => func!(_glTextureSubImage1D),
            "_emscripten_glTextureSubImage2D" => func!(_glTextureSubImage2D),
            "_glTextureSubImage2D" => func!(_glTextureSubImage2D),
            "_emscripten_glTextureSubImage3D" => func!(_glTextureSubImage3D),
            "_glTextureSubImage3D" => func!(_glTextureSubImage3D),
            "_emscripten_glTextureView" => func!(_glTextureView),
            "_glTextureView" => func!(_glTextureView),
            "_emscripten_glTransformFeedbackBufferBase" => func!(_glTransformFeedbackBufferBase),
            "_glTransformFeedbackBufferBase" => func!(_glTransformFeedbackBufferBase),
            "_emscripten_glTransformFeedbackBufferRange" => func!(_glTransformFeedbackBufferRange),
            "_glTransformFeedbackBufferRange" => func!(_glTransformFeedbackBufferRange),
            "_emscripten_glTransformFeedbackVaryings" => func!(_glTransformFeedbackVaryings),
            "_glTransformFeedbackVaryings" => func!(_glTransformFeedbackVaryings),
            "_emscripten_glUniform1d" => func!(_glUniform1d),
            "_glUniform1d" => func!(_glUniform1d),
            "_emscripten_glUniform1dv" => func!(_glUniform1dv),
            "_glUniform1dv" => func!(_glUniform1dv),
            "_emscripten_glUniform1f" => func!(_glUniform1f),
            "_glUniform1f" => func!(_glUniform1f),
            "_emscripten_glUniform1fv" => func!(_glUniform1fv),
            "_glUniform1fv" => func!(_glUniform1fv),
            "_emscripten_glUniform1i" => func!(_glUniform1i),
            "_glUniform1i" => func!(_glUniform1i),
            "_emscripten_glUniform1iv" => func!(_glUniform1iv),
            "_glUniform1iv" => func!(_glUniform1iv),
            "_emscripten_glUniform1ui" => func!(_glUniform1ui),
            "_glUniform1ui" => func!(_glUniform1ui),
            "_emscripten_glUniform1uiv" => func!(_glUniform1uiv),
            "_glUniform1uiv" => func!(_glUniform1uiv),
            "_emscripten_glUniform2d" => func!(_glUniform2d),
            "_glUniform2d" => func!(_glUniform2d),
            "_emscripten_glUniform2dv" => func!(_glUniform2dv),
            "_glUniform2dv" => func!(_glUniform2dv),
            "_emscripten_glUniform2f" => func!(_glUniform2f),
            "_glUniform2f" => func!(_glUniform2f),
            "_emscripten_glUniform2fv" => func!(_glUniform2fv),
            "_glUniform2fv" => func!(_glUniform2fv),
            "_emscripten_glUniform2i" => func!(_glUniform2i),
            "_glUniform2i" => func!(_glUniform2i),
            "_emscripten_glUniform2iv" => func!(_glUniform2iv),
            "_glUniform2iv" => func!(_glUniform2iv),
            "_emscripten_glUniform2ui" => func!(_glUniform2ui),
            "_glUniform2ui" => func!(_glUniform2ui),
            "_emscripten_glUniform2uiv" => func!(_glUniform2uiv),
            "_glUniform2uiv" => func!(_glUniform2uiv),
            "_emscripten_glUniform3d" => func!(_glUniform3d),
            "_glUniform3d" => func!(_glUniform3d),
            "_emscripten_glUniform3dv" => func!(_glUniform3dv),
            "_glUniform3dv" => func!(_glUniform3dv),
            "_emscripten_glUniform3f" => func!(_glUniform3f),
            "_glUniform3f" => func!(_glUniform3f),
            "_emscripten_glUniform3fv" => func!(_glUniform3fv),
            "_glUniform3fv" => func!(_glUniform3fv),
            "_emscripten_glUniform3i" => func!(_glUniform3i),
            "_glUniform3i" => func!(_glUniform3i),
            "_emscripten_glUniform3iv" => func!(_glUniform3iv),
            "_glUniform3iv" => func!(_glUniform3iv),
            "_emscripten_glUniform3ui" => func!(_glUniform3ui),
            "_glUniform3ui" => func!(_glUniform3ui),
            "_emscripten_glUniform3uiv" => func!(_glUniform3uiv),
            "_glUniform3uiv" => func!(_glUniform3uiv),
            "_emscripten_glUniform4d" => func!(_glUniform4d),
            "_glUniform4d" => func!(_glUniform4d),
            "_emscripten_glUniform4dv" => func!(_glUniform4dv),
            "_glUniform4dv" => func!(_glUniform4dv),
            "_emscripten_glUniform4f" => func!(_glUniform4f),
            "_glUniform4f" => func!(_glUniform4f),
            "_emscripten_glUniform4fv" => func!(_glUniform4fv),
            "_glUniform4fv" => func!(_glUniform4fv),
            "_emscripten_glUniform4i" => func!(_glUniform4i),
            "_glUniform4i" => func!(_glUniform4i),
            "_emscripten_glUniform4iv" => func!(_glUniform4iv),
            "_glUniform4iv" => func!(_glUniform4iv),
            "_emscripten_glUniform4ui" => func!(_glUniform4ui),
            "_glUniform4ui" => func!(_glUniform4ui),
            "_emscripten_glUniform4uiv" => func!(_glUniform4uiv),
            "_glUniform4uiv" => func!(_glUniform4uiv),
            "_emscripten_glUniformBlockBinding" => func!(_glUniformBlockBinding),
            "_glUniformBlockBinding" => func!(_glUniformBlockBinding),
            "_emscripten_glUniformMatrix2dv" => func!(_glUniformMatrix2dv),
            "_glUniformMatrix2dv" => func!(_glUniformMatrix2dv),
            "_emscripten_glUniformMatrix2fv" => func!(_glUniformMatrix2fv),
            "_glUniformMatrix2fv" => func!(_glUniformMatrix2fv),
            "_emscripten_glUniformMatrix2x3dv" => func!(_glUniformMatrix2x3dv),
            "_glUniformMatrix2x3dv" => func!(_glUniformMatrix2x3dv),
            "_emscripten_glUniformMatrix2x3fv" => func!(_glUniformMatrix2x3fv),
            "_glUniformMatrix2x3fv" => func!(_glUniformMatrix2x3fv),
            "_emscripten_glUniformMatrix2x4dv" => func!(_glUniformMatrix2x4dv),
            "_glUniformMatrix2x4dv" => func!(_glUniformMatrix2x4dv),
            "_emscripten_glUniformMatrix2x4fv" => func!(_glUniformMatrix2x4fv),
            "_glUniformMatrix2x4fv" => func!(_glUniformMatrix2x4fv),
            "_emscripten_glUniformMatrix3dv" => func!(_glUniformMatrix3dv),
            "_glUniformMatrix3dv" => func!(_glUniformMatrix3dv),
            "_emscripten_glUniformMatrix3fv" => func!(_glUniformMatrix3fv),
            "_glUniformMatrix3fv" => func!(_glUniformMatrix3fv),
            "_emscripten_glUniformMatrix3x2dv" => func!(_glUniformMatrix3x2dv),
            "_glUniformMatrix3x2dv" => func!(_glUniformMatrix3x2dv),
            "_emscripten_glUniformMatrix3x2fv" => func!(_glUniformMatrix3x2fv),
            "_glUniformMatrix3x2fv" => func!(_glUniformMatrix3x2fv),
            "_emscripten_glUniformMatrix3x4dv" => func!(_glUniformMatrix3x4dv),
            "_glUniformMatrix3x4dv" => func!(_glUniformMatrix3x4dv),
            "_emscripten_glUniformMatrix3x4fv" => func!(_glUniformMatrix3x4fv),
            "_glUniformMatrix3x4fv" => func!(_glUniformMatrix3x4fv),
            "_emscripten_glUniformMatrix4dv" => func!(_glUniformMatrix4dv),
            "_glUniformMatrix4dv" => func!(_glUniformMatrix4dv),
            "_emscripten_glUniformMatrix4fv" => func!(_glUniformMatrix4fv),
            "_glUniformMatrix4fv" => func!(_glUniformMatrix4fv),
            "_emscripten_glUniformMatrix4x2dv" => func!(_glUniformMatrix4x2dv),
            "_glUniformMatrix4x2dv" => func!(_glUniformMatrix4x2dv),
            "_emscripten_glUniformMatrix4x2fv" => func!(_glUniformMatrix4x2fv),
            "_glUniformMatrix4x2fv" => func!(_glUniformMatrix4x2fv),
            "_emscripten_glUniformMatrix4x3dv" => func!(_glUniformMatrix4x3dv),
            "_glUniformMatrix4x3dv" => func!(_glUniformMatrix4x3dv),
            "_emscripten_glUniformMatrix4x3fv" => func!(_glUniformMatrix4x3fv),
            "_glUniformMatrix4x3fv" => func!(_glUniformMatrix4x3fv),
            "_emscripten_glUniformSubroutinesuiv" => func!(_glUniformSubroutinesuiv),
            "_glUniformSubroutinesuiv" => func!(_glUniformSubroutinesuiv),
            "_emscripten_glUnmapBuffer" => func!(_glUnmapBuffer),
            "_glUnmapBuffer" => func!(_glUnmapBuffer),
            "_emscripten_glUnmapNamedBuffer" => func!(_glUnmapNamedBuffer),
            "_glUnmapNamedBuffer" => func!(_glUnmapNamedBuffer),
            "_emscripten_glUseProgram" => func!(_glUseProgram),
            "_glUseProgram" => func!(_glUseProgram),
            "_emscripten_glUseProgramStages" => func!(_glUseProgramStages),
            "_glUseProgramStages" => func!(_glUseProgramStages),
            "_emscripten_glValidateProgram" => func!(_glValidateProgram),
            "_glValidateProgram" => func!(_glValidateProgram),
            "_emscripten_glValidateProgramPipeline" => func!(_glValidateProgramPipeline),
            "_glValidateProgramPipeline" => func!(_glValidateProgramPipeline),
            "_emscripten_glVertexArrayAttribBinding" => func!(_glVertexArrayAttribBinding),
            "_glVertexArrayAttribBinding" => func!(_glVertexArrayAttribBinding),
            "_emscripten_glVertexArrayAttribFormat" => func!(_glVertexArrayAttribFormat),
            "_glVertexArrayAttribFormat" => func!(_glVertexArrayAttribFormat),
            "_emscripten_glVertexArrayAttribIFormat" => func!(_glVertexArrayAttribIFormat),
            "_glVertexArrayAttribIFormat" => func!(_glVertexArrayAttribIFormat),
            "_emscripten_glVertexArrayAttribLFormat" => func!(_glVertexArrayAttribLFormat),
            "_glVertexArrayAttribLFormat" => func!(_glVertexArrayAttribLFormat),
            "_emscripten_glVertexArrayBindingDivisor" => func!(_glVertexArrayBindingDivisor),
            "_glVertexArrayBindingDivisor" => func!(_glVertexArrayBindingDivisor),
            "_emscripten_glVertexArrayElementBuffer" => func!(_glVertexArrayElementBuffer),
            "_glVertexArrayElementBuffer" => func!(_glVertexArrayElementBuffer),
            "_emscripten_glVertexArrayVertexBuffer" => func!(_glVertexArrayVertexBuffer),
            "_glVertexArrayVertexBuffer" => func!(_glVertexArrayVertexBuffer),
            "_emscripten_glVertexArrayVertexBuffers" => func!(_glVertexArrayVertexBuffers),
            "_glVertexArrayVertexBuffers" => func!(_glVertexArrayVertexBuffers),
            "_emscripten_glVertexAttrib1d" => func!(_glVertexAttrib1d),
            "_glVertexAttrib1d" => func!(_glVertexAttrib1d),
            "_emscripten_glVertexAttrib1dv" => func!(_glVertexAttrib1dv),
            "_glVertexAttrib1dv" => func!(_glVertexAttrib1dv),
            "_emscripten_glVertexAttrib1f" => func!(_glVertexAttrib1f),
            "_glVertexAttrib1f" => func!(_glVertexAttrib1f),
            "_emscripten_glVertexAttrib1fv" => func!(_glVertexAttrib1fv),
            "_glVertexAttrib1fv" => func!(_glVertexAttrib1fv),
            "_emscripten_glVertexAttrib1s" => func!(_glVertexAttrib1s),
            "_glVertexAttrib1s" => func!(_glVertexAttrib1s),
            "_emscripten_glVertexAttrib1sv" => func!(_glVertexAttrib1sv),
            "_glVertexAttrib1sv" => func!(_glVertexAttrib1sv),
            "_emscripten_glVertexAttrib2d" => func!(_glVertexAttrib2d),
            "_glVertexAttrib2d" => func!(_glVertexAttrib2d),
            "_emscripten_glVertexAttrib2dv" => func!(_glVertexAttrib2dv),
            "_glVertexAttrib2dv" => func!(_glVertexAttrib2dv),
            "_emscripten_glVertexAttrib2f" => func!(_glVertexAttrib2f),
            "_glVertexAttrib2f" => func!(_glVertexAttrib2f),
            "_emscripten_glVertexAttrib2fv" => func!(_glVertexAttrib2fv),
            "_glVertexAttrib2fv" => func!(_glVertexAttrib2fv),
            "_emscripten_glVertexAttrib2s" => func!(_glVertexAttrib2s),
            "_glVertexAttrib2s" => func!(_glVertexAttrib2s),
            "_emscripten_glVertexAttrib2sv" => func!(_glVertexAttrib2sv),
            "_glVertexAttrib2sv" => func!(_glVertexAttrib2sv),
            "_emscripten_glVertexAttrib3d" => func!(_glVertexAttrib3d),
            "_glVertexAttrib3d" => func!(_glVertexAttrib3d),
            "_emscripten_glVertexAttrib3dv" => func!(_glVertexAttrib3dv),
            "_glVertexAttrib3dv" => func!(_glVertexAttrib3dv),
            "_emscripten_glVertexAttrib3f" => func!(_glVertexAttrib3f),
            "_glVertexAttrib3f" => func!(_glVertexAttrib3f),
            "_emscripten_glVertexAttrib3fv" => func!(_glVertexAttrib3fv),
            "_glVertexAttrib3fv" => func!(_glVertexAttrib3fv),
            "_emscripten_glVertexAttrib3s" => func!(_glVertexAttrib3s),
            "_glVertexAttrib3s" => func!(_glVertexAttrib3s),
            "_emscripten_glVertexAttrib3sv" => func!(_glVertexAttrib3sv),
            "_glVertexAttrib3sv" => func!(_glVertexAttrib3sv),
            "_emscripten_glVertexAttrib4Nbv" => func!(_glVertexAttrib4Nbv),
            "_glVertexAttrib4Nbv" => func!(_glVertexAttrib4Nbv),
            "_emscripten_glVertexAttrib4Niv" => func!(_glVertexAttrib4Niv),
            "_glVertexAttrib4Niv" => func!(_glVertexAttrib4Niv),
            "_emscripten_glVertexAttrib4Nsv" => func!(_glVertexAttrib4Nsv),
            "_glVertexAttrib4Nsv" => func!(_glVertexAttrib4Nsv),
            "_emscripten_glVertexAttrib4Nub" => func!(_glVertexAttrib4Nub),
            "_glVertexAttrib4Nub" => func!(_glVertexAttrib4Nub),
            "_emscripten_glVertexAttrib4Nubv" => func!(_glVertexAttrib4Nubv),
            "_glVertexAttrib4Nubv" => func!(_glVertexAttrib4Nubv),
            "_emscripten_glVertexAttrib4Nuiv" => func!(_glVertexAttrib4Nuiv),
            "_glVertexAttrib4Nuiv" => func!(_glVertexAttrib4Nuiv),
            "_emscripten_glVertexAttrib4Nusv" => func!(_glVertexAttrib4Nusv),
            "_glVertexAttrib4Nusv" => func!(_glVertexAttrib4Nusv),
            "_emscripten_glVertexAttrib4bv" => func!(_glVertexAttrib4bv),
            "_glVertexAttrib4bv" => func!(_glVertexAttrib4bv),
            "_emscripten_glVertexAttrib4d" => func!(_glVertexAttrib4d),
            "_glVertexAttrib4d" => func!(_glVertexAttrib4d),
            "_emscripten_glVertexAttrib4dv" => func!(_glVertexAttrib4dv),
            "_glVertexAttrib4dv" => func!(_glVertexAttrib4dv),
            "_emscripten_glVertexAttrib4f" => func!(_glVertexAttrib4f),
            "_glVertexAttrib4f" => func!(_glVertexAttrib4f),
            "_emscripten_glVertexAttrib4fv" => func!(_glVertexAttrib4fv),
            "_glVertexAttrib4fv" => func!(_glVertexAttrib4fv),
            "_emscripten_glVertexAttrib4iv" => func!(_glVertexAttrib4iv),
            "_glVertexAttrib4iv" => func!(_glVertexAttrib4iv),
            "_emscripten_glVertexAttrib4s" => func!(_glVertexAttrib4s),
            "_glVertexAttrib4s" => func!(_glVertexAttrib4s),
            "_emscripten_glVertexAttrib4sv" => func!(_glVertexAttrib4sv),
            "_glVertexAttrib4sv" => func!(_glVertexAttrib4sv),
            "_emscripten_glVertexAttrib4ubv" => func!(_glVertexAttrib4ubv),
            "_glVertexAttrib4ubv" => func!(_glVertexAttrib4ubv),
            "_emscripten_glVertexAttrib4uiv" => func!(_glVertexAttrib4uiv),
            "_glVertexAttrib4uiv" => func!(_glVertexAttrib4uiv),
            "_emscripten_glVertexAttrib4usv" => func!(_glVertexAttrib4usv),
            "_glVertexAttrib4usv" => func!(_glVertexAttrib4usv),
            "_emscripten_glVertexAttribBinding" => func!(_glVertexAttribBinding),
            "_glVertexAttribBinding" => func!(_glVertexAttribBinding),
            "_emscripten_glVertexAttribDivisor" => func!(_glVertexAttribDivisor),
            "_glVertexAttribDivisor" => func!(_glVertexAttribDivisor),
            "_emscripten_glVertexAttribFormat" => func!(_glVertexAttribFormat),
            "_glVertexAttribFormat" => func!(_glVertexAttribFormat),
            "_emscripten_glVertexAttribI1i" => func!(_glVertexAttribI1i),
            "_glVertexAttribI1i" => func!(_glVertexAttribI1i),
            "_emscripten_glVertexAttribI1iv" => func!(_glVertexAttribI1iv),
            "_glVertexAttribI1iv" => func!(_glVertexAttribI1iv),
            "_emscripten_glVertexAttribI1ui" => func!(_glVertexAttribI1ui),
            "_glVertexAttribI1ui" => func!(_glVertexAttribI1ui),
            "_emscripten_glVertexAttribI1uiv" => func!(_glVertexAttribI1uiv),
            "_glVertexAttribI1uiv" => func!(_glVertexAttribI1uiv),
            "_emscripten_glVertexAttribI2i" => func!(_glVertexAttribI2i),
            "_glVertexAttribI2i" => func!(_glVertexAttribI2i),
            "_emscripten_glVertexAttribI2iv" => func!(_glVertexAttribI2iv),
            "_glVertexAttribI2iv" => func!(_glVertexAttribI2iv),
            "_emscripten_glVertexAttribI2ui" => func!(_glVertexAttribI2ui),
            "_glVertexAttribI2ui" => func!(_glVertexAttribI2ui),
            "_emscripten_glVertexAttribI2uiv" => func!(_glVertexAttribI2uiv),
            "_glVertexAttribI2uiv" => func!(_glVertexAttribI2uiv),
            "_emscripten_glVertexAttribI3i" => func!(_glVertexAttribI3i),
            "_glVertexAttribI3i" => func!(_glVertexAttribI3i),
            "_emscripten_glVertexAttribI3iv" => func!(_glVertexAttribI3iv),
            "_glVertexAttribI3iv" => func!(_glVertexAttribI3iv),
            "_emscripten_glVertexAttribI3ui" => func!(_glVertexAttribI3ui),
            "_glVertexAttribI3ui" => func!(_glVertexAttribI3ui),
            "_emscripten_glVertexAttribI3uiv" => func!(_glVertexAttribI3uiv),
            "_glVertexAttribI3uiv" => func!(_glVertexAttribI3uiv),
            "_emscripten_glVertexAttribI4bv" => func!(_glVertexAttribI4bv),
            "_glVertexAttribI4bv" => func!(_glVertexAttribI4bv),
            "_emscripten_glVertexAttribI4i" => func!(_glVertexAttribI4i),
            "_glVertexAttribI4i" => func!(_glVertexAttribI4i),
            "_emscripten_glVertexAttribI4iv" => func!(_glVertexAttribI4iv),
            "_glVertexAttribI4iv" => func!(_glVertexAttribI4iv),
            "_emscripten_glVertexAttribI4sv" => func!(_glVertexAttribI4sv),
            "_glVertexAttribI4sv" => func!(_glVertexAttribI4sv),
            "_emscripten_glVertexAttribI4ubv" => func!(_glVertexAttribI4ubv),
            "_glVertexAttribI4ubv" => func!(_glVertexAttribI4ubv),
            "_emscripten_glVertexAttribI4ui" => func!(_glVertexAttribI4ui),
            "_glVertexAttribI4ui" => func!(_glVertexAttribI4ui),
            "_emscripten_glVertexAttribI4uiv" => func!(_glVertexAttribI4uiv),
            "_glVertexAttribI4uiv" => func!(_glVertexAttribI4uiv),
            "_emscripten_glVertexAttribI4usv" => func!(_glVertexAttribI4usv),
            "_glVertexAttribI4usv" => func!(_glVertexAttribI4usv),
            "_emscripten_glVertexAttribIFormat" => func!(_glVertexAttribIFormat),
            "_glVertexAttribIFormat" => func!(_glVertexAttribIFormat),
            "_emscripten_glVertexAttribIPointer" => func!(_glVertexAttribIPointer),
            "_glVertexAttribIPointer" => func!(_glVertexAttribIPointer),
            "_emscripten_glVertexAttribL1d" => func!(_glVertexAttribL1d),
            "_glVertexAttribL1d" => func!(_glVertexAttribL1d),
            "_emscripten_glVertexAttribL1dv" => func!(_glVertexAttribL1dv),
            "_glVertexAttribL1dv" => func!(_glVertexAttribL1dv),
            "_emscripten_glVertexAttribL2d" => func!(_glVertexAttribL2d),
            "_glVertexAttribL2d" => func!(_glVertexAttribL2d),
            "_emscripten_glVertexAttribL2dv" => func!(_glVertexAttribL2dv),
            "_glVertexAttribL2dv" => func!(_glVertexAttribL2dv),
            "_emscripten_glVertexAttribL3d" => func!(_glVertexAttribL3d),
            "_glVertexAttribL3d" => func!(_glVertexAttribL3d),
            "_emscripten_glVertexAttribL3dv" => func!(_glVertexAttribL3dv),
            "_glVertexAttribL3dv" => func!(_glVertexAttribL3dv),
            "_emscripten_glVertexAttribL4d" => func!(_glVertexAttribL4d),
            "_glVertexAttribL4d" => func!(_glVertexAttribL4d),
            "_emscripten_glVertexAttribL4dv" => func!(_glVertexAttribL4dv),
            "_glVertexAttribL4dv" => func!(_glVertexAttribL4dv),
            "_emscripten_glVertexAttribLFormat" => func!(_glVertexAttribLFormat),
            "_glVertexAttribLFormat" => func!(_glVertexAttribLFormat),
            "_emscripten_glVertexAttribLPointer" => func!(_glVertexAttribLPointer),
            "_glVertexAttribLPointer" => func!(_glVertexAttribLPointer),
            "_emscripten_glVertexAttribP1ui" => func!(_glVertexAttribP1ui),
            "_glVertexAttribP1ui" => func!(_glVertexAttribP1ui),
            "_emscripten_glVertexAttribP1uiv" => func!(_glVertexAttribP1uiv),
            "_glVertexAttribP1uiv" => func!(_glVertexAttribP1uiv),
            "_emscripten_glVertexAttribP2ui" => func!(_glVertexAttribP2ui),
            "_glVertexAttribP2ui" => func!(_glVertexAttribP2ui),
            "_emscripten_glVertexAttribP2uiv" => func!(_glVertexAttribP2uiv),
            "_glVertexAttribP2uiv" => func!(_glVertexAttribP2uiv),
            "_emscripten_glVertexAttribP3ui" => func!(_glVertexAttribP3ui),
            "_glVertexAttribP3ui" => func!(_glVertexAttribP3ui),
            "_emscripten_glVertexAttribP3uiv" => func!(_glVertexAttribP3uiv),
            "_glVertexAttribP3uiv" => func!(_glVertexAttribP3uiv),
            "_emscripten_glVertexAttribP4ui" => func!(_glVertexAttribP4ui),
            "_glVertexAttribP4ui" => func!(_glVertexAttribP4ui),
            "_emscripten_glVertexAttribP4uiv" => func!(_glVertexAttribP4uiv),
            "_glVertexAttribP4uiv" => func!(_glVertexAttribP4uiv),
            "_emscripten_glVertexAttribPointer" => func!(_glVertexAttribPointer),
            "_glVertexAttribPointer" => func!(_glVertexAttribPointer),
            "_emscripten_glVertexBindingDivisor" => func!(_glVertexBindingDivisor),
            "_glVertexBindingDivisor" => func!(_glVertexBindingDivisor),
            "_emscripten_glVertexP2ui" => func!(_glVertexP2ui),
            "_glVertexP2ui" => func!(_glVertexP2ui),
            "_emscripten_glVertexP2uiv" => func!(_glVertexP2uiv),
            "_glVertexP2uiv" => func!(_glVertexP2uiv),
            "_emscripten_glVertexP3ui" => func!(_glVertexP3ui),
            "_glVertexP3ui" => func!(_glVertexP3ui),
            "_emscripten_glVertexP3uiv" => func!(_glVertexP3uiv),
            "_glVertexP3uiv" => func!(_glVertexP3uiv),
            "_emscripten_glVertexP4ui" => func!(_glVertexP4ui),
            "_glVertexP4ui" => func!(_glVertexP4ui),
            "_emscripten_glVertexP4uiv" => func!(_glVertexP4uiv),
            "_glVertexP4uiv" => func!(_glVertexP4uiv),
            "_emscripten_glViewport" => func!(_glViewport),
            "_glViewport" => func!(_glViewport),
            "_emscripten_glViewportArrayv" => func!(_glViewportArrayv),
            "_glViewportArrayv" => func!(_glViewportArrayv),
            "_emscripten_glViewportIndexedf" => func!(_glViewportIndexedf),
            "_glViewportIndexedf" => func!(_glViewportIndexedf),
            "_emscripten_glViewportIndexedfv" => func!(_glViewportIndexedfv),
            "_glViewportIndexedfv" => func!(_glViewportIndexedfv),
            "_emscripten_glWaitSync" => func!(_glWaitSync),
            "_glWaitSync" => func!(_glWaitSync),
            },
            }
        }
        pub fn _glActiveShaderProgram(
            _ctx: &mut Ctx,
            pipeline: types::GLuint,
            program: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::ActiveShaderProgram(pipeline, program) };
            _res
        }
        pub fn _glActiveTexture(_ctx: &mut Ctx, texture: types::GLenum) -> () {
            let _res = unsafe { gl::ActiveTexture(texture) };
            _res
        }
        pub fn _glAttachShader(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shader: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::AttachShader(program, shader) };
            _res
        }
        pub fn _glBeginConditionalRender(
            _ctx: &mut Ctx,
            id: types::GLuint,
            mode: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::BeginConditionalRender(id, mode) };
            _res
        }
        pub fn _glBeginQuery(_ctx: &mut Ctx, target: types::GLenum, id: types::GLuint) -> () {
            let _res = unsafe { gl::BeginQuery(target, id) };
            _res
        }
        pub fn _glBeginQueryIndexed(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            id: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::BeginQueryIndexed(target, index, id) };
            _res
        }
        pub fn _glBeginTransformFeedback(_ctx: &mut Ctx, primitiveMode: types::GLenum) -> () {
            let _res = unsafe { gl::BeginTransformFeedback(primitiveMode) };
            _res
        }
        pub fn _glBindAttribLocation(
            _ctx: &mut Ctx,
            program: types::GLuint,
            index: types::GLuint,
            name: u32,
        ) -> () {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::BindAttribLocation(program, index, name) };
            _res
        }
        pub fn _glBindBuffer(_ctx: &mut Ctx, target: types::GLenum, buffer: types::GLuint) -> () {
            let _res = unsafe { gl::BindBuffer(target, buffer) };
            _res
        }
        pub fn _glBindBufferBase(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            buffer: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::BindBufferBase(target, index, buffer) };
            _res
        }
        pub fn _glBindBufferRange(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            buffer: types::GLuint,
            offset: u32,
            size: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let _res = unsafe { gl::BindBufferRange(target, index, buffer, offset, size) };
            _res
        }
        pub fn _glBindBuffersBase(
            _ctx: &mut Ctx,
            target: types::GLenum,
            first: types::GLuint,
            count: types::GLsizei,
            buffers: u32,
        ) -> () {
            let buffers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), buffers) as *const types::GLuint;
            let _res = unsafe { gl::BindBuffersBase(target, first, count, buffers) };
            _res
        }
        pub fn _glBindBuffersRange(
            _ctx: &mut Ctx,
            target: types::GLenum,
            first: types::GLuint,
            count: types::GLsizei,
            buffers: u32,
            offsets: u32,
            sizes: u32,
        ) -> () {
            let buffers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), buffers) as *const types::GLuint;
            let offsets: *const types::GLintptr =
                emscripten_memory_pointer!(_ctx.memory(0), offsets) as *const types::GLintptr;
            let sizes: *const types::GLsizeiptr =
                emscripten_memory_pointer!(_ctx.memory(0), sizes) as *const types::GLsizeiptr;
            let _res =
                unsafe { gl::BindBuffersRange(target, first, count, buffers, offsets, sizes) };
            _res
        }
        pub fn _glBindFragDataLocation(
            _ctx: &mut Ctx,
            program: types::GLuint,
            color: types::GLuint,
            name: u32,
        ) -> () {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::BindFragDataLocation(program, color, name) };
            _res
        }
        pub fn _glBindFragDataLocationIndexed(
            _ctx: &mut Ctx,
            program: types::GLuint,
            colorNumber: types::GLuint,
            index: types::GLuint,
            name: u32,
        ) -> () {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res =
                unsafe { gl::BindFragDataLocationIndexed(program, colorNumber, index, name) };
            _res
        }
        pub fn _glBindFramebuffer(
            _ctx: &mut Ctx,
            target: types::GLenum,
            framebuffer: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::BindFramebuffer(target, framebuffer) };
            _res
        }
        pub fn _glBindImageTexture(
            _ctx: &mut Ctx,
            unit: types::GLuint,
            texture: types::GLuint,
            level: types::GLint,
            layered: types::GLboolean,
            layer: types::GLint,
            access: types::GLenum,
            format: types::GLenum,
        ) -> () {
            let _res = unsafe {
                gl::BindImageTexture(unit, texture, level, layered, layer, access, format)
            };
            _res
        }
        pub fn _glBindImageTextures(
            _ctx: &mut Ctx,
            first: types::GLuint,
            count: types::GLsizei,
            textures: u32,
        ) -> () {
            let textures: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), textures) as *const types::GLuint;
            let _res = unsafe { gl::BindImageTextures(first, count, textures) };
            _res
        }
        pub fn _glBindProgramPipeline(_ctx: &mut Ctx, pipeline: types::GLuint) -> () {
            let _res = unsafe { gl::BindProgramPipeline(pipeline) };
            _res
        }
        pub fn _glBindRenderbuffer(
            _ctx: &mut Ctx,
            target: types::GLenum,
            renderbuffer: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::BindRenderbuffer(target, renderbuffer) };
            _res
        }
        pub fn _glBindSampler(_ctx: &mut Ctx, unit: types::GLuint, sampler: types::GLuint) -> () {
            let _res = unsafe { gl::BindSampler(unit, sampler) };
            _res
        }
        pub fn _glBindSamplers(
            _ctx: &mut Ctx,
            first: types::GLuint,
            count: types::GLsizei,
            samplers: u32,
        ) -> () {
            let samplers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), samplers) as *const types::GLuint;
            let _res = unsafe { gl::BindSamplers(first, count, samplers) };
            _res
        }
        pub fn _glBindTexture(_ctx: &mut Ctx, target: types::GLenum, texture: types::GLuint) -> () {
            let _res = unsafe { gl::BindTexture(target, texture) };
            _res
        }
        pub fn _glBindTextureUnit(
            _ctx: &mut Ctx,
            unit: types::GLuint,
            texture: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::BindTextureUnit(unit, texture) };
            _res
        }
        pub fn _glBindTextures(
            _ctx: &mut Ctx,
            first: types::GLuint,
            count: types::GLsizei,
            textures: u32,
        ) -> () {
            let textures: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), textures) as *const types::GLuint;
            let _res = unsafe { gl::BindTextures(first, count, textures) };
            _res
        }
        pub fn _glBindTransformFeedback(
            _ctx: &mut Ctx,
            target: types::GLenum,
            id: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::BindTransformFeedback(target, id) };
            _res
        }
        pub fn _glBindVertexArray(_ctx: &mut Ctx, array: types::GLuint) -> () {
            let _res = unsafe { gl::BindVertexArray(array) };
            _res
        }
        pub fn _glBindVertexBuffer(
            _ctx: &mut Ctx,
            bindingindex: types::GLuint,
            buffer: types::GLuint,
            offset: u32,
            stride: types::GLsizei,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;

            let _res = unsafe { gl::BindVertexBuffer(bindingindex, buffer, offset, stride) };
            _res
        }
        pub fn _glBindVertexBuffers(
            _ctx: &mut Ctx,
            first: types::GLuint,
            count: types::GLsizei,
            buffers: u32,
            offsets: u32,
            strides: u32,
        ) -> () {
            let buffers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), buffers) as *const types::GLuint;
            let offsets: *const types::GLintptr =
                emscripten_memory_pointer!(_ctx.memory(0), offsets) as *const types::GLintptr;
            let strides: *const types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), strides) as *const types::GLsizei;
            let _res = unsafe { gl::BindVertexBuffers(first, count, buffers, offsets, strides) };
            _res
        }
        pub fn _glBlendColor(_ctx: &mut Ctx, red: f64, green: f64, blue: f64, alpha: f64) -> () {
            let red: types::GLfloat = red as types::GLfloat;
            let green: types::GLfloat = green as types::GLfloat;
            let blue: types::GLfloat = blue as types::GLfloat;
            let alpha: types::GLfloat = alpha as types::GLfloat;
            let _res = unsafe { gl::BlendColor(red, green, blue, alpha) };
            _res
        }
        pub fn _glBlendEquation(_ctx: &mut Ctx, mode: types::GLenum) -> () {
            let _res = unsafe { gl::BlendEquation(mode) };
            _res
        }
        pub fn _glBlendEquationSeparate(
            _ctx: &mut Ctx,
            modeRGB: types::GLenum,
            modeAlpha: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::BlendEquationSeparate(modeRGB, modeAlpha) };
            _res
        }
        pub fn _glBlendEquationSeparatei(
            _ctx: &mut Ctx,
            buf: types::GLuint,
            modeRGB: types::GLenum,
            modeAlpha: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::BlendEquationSeparatei(buf, modeRGB, modeAlpha) };
            _res
        }
        pub fn _glBlendEquationi(_ctx: &mut Ctx, buf: types::GLuint, mode: types::GLenum) -> () {
            let _res = unsafe { gl::BlendEquationi(buf, mode) };
            _res
        }
        pub fn _glBlendFunc(_ctx: &mut Ctx, sfactor: types::GLenum, dfactor: types::GLenum) -> () {
            let _res = unsafe { gl::BlendFunc(sfactor, dfactor) };
            _res
        }
        pub fn _glBlendFuncSeparate(
            _ctx: &mut Ctx,
            sfactorRGB: types::GLenum,
            dfactorRGB: types::GLenum,
            sfactorAlpha: types::GLenum,
            dfactorAlpha: types::GLenum,
        ) -> () {
            let _res = unsafe {
                gl::BlendFuncSeparate(sfactorRGB, dfactorRGB, sfactorAlpha, dfactorAlpha)
            };
            _res
        }
        pub fn _glBlendFuncSeparatei(
            _ctx: &mut Ctx,
            buf: types::GLuint,
            srcRGB: types::GLenum,
            dstRGB: types::GLenum,
            srcAlpha: types::GLenum,
            dstAlpha: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::BlendFuncSeparatei(buf, srcRGB, dstRGB, srcAlpha, dstAlpha) };
            _res
        }
        pub fn _glBlendFunci(
            _ctx: &mut Ctx,
            buf: types::GLuint,
            src: types::GLenum,
            dst: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::BlendFunci(buf, src, dst) };
            _res
        }
        pub fn _glBlitFramebuffer(
            _ctx: &mut Ctx,
            srcX0: types::GLint,
            srcY0: types::GLint,
            srcX1: types::GLint,
            srcY1: types::GLint,
            dstX0: types::GLint,
            dstY0: types::GLint,
            dstX1: types::GLint,
            dstY1: types::GLint,
            mask: types::GLbitfield,
            filter: types::GLenum,
        ) -> () {
            let _res = unsafe {
                gl::BlitFramebuffer(
                    srcX0, srcY0, srcX1, srcY1, dstX0, dstY0, dstX1, dstY1, mask, filter,
                )
            };
            _res
        }
        pub fn _glBlitNamedFramebuffer(
            _ctx: &mut Ctx,
            readFramebuffer: types::GLuint,
            drawFramebuffer: types::GLuint,
            srcX0: types::GLint,
            srcY0: types::GLint,
            srcX1: types::GLint,
            srcY1: types::GLint,
            dstX0: types::GLint,
            dstY0: types::GLint,
            dstX1: types::GLint,
            dstY1: types::GLint,
            mask: types::GLbitfield,
            filter: types::GLenum,
        ) -> () {
            let _res = unsafe {
                gl::BlitNamedFramebuffer(
                    readFramebuffer,
                    drawFramebuffer,
                    srcX0,
                    srcY0,
                    srcX1,
                    srcY1,
                    dstX0,
                    dstY0,
                    dstX1,
                    dstY1,
                    mask,
                    filter,
                )
            };
            _res
        }
        pub fn _glBufferData(
            _ctx: &mut Ctx,
            target: types::GLenum,
            size: u32,
            data: u32,
            usage: types::GLenum,
        ) -> () {
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::BufferData(target, size, data, usage) };
            _res
        }
        pub fn _glBufferStorage(
            _ctx: &mut Ctx,
            target: types::GLenum,
            size: u32,
            data: u32,
            flags: types::GLbitfield,
        ) -> () {
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::BufferStorage(target, size, data, flags) };
            _res
        }
        pub fn _glBufferSubData(
            _ctx: &mut Ctx,
            target: types::GLenum,
            offset: u32,
            size: u32,
            data: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::BufferSubData(target, offset, size, data) };
            _res
        }
        pub fn _glCheckFramebufferStatus(_ctx: &mut Ctx, target: types::GLenum) -> types::GLenum {
            let _res = unsafe { gl::CheckFramebufferStatus(target) };
            _res
        }
        pub fn _glCheckNamedFramebufferStatus(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            target: types::GLenum,
        ) -> types::GLenum {
            let _res = unsafe { gl::CheckNamedFramebufferStatus(framebuffer, target) };
            _res
        }
        pub fn _glClampColor(_ctx: &mut Ctx, target: types::GLenum, clamp: types::GLenum) -> () {
            let _res = unsafe { gl::ClampColor(target, clamp) };
            _res
        }
        pub fn _glClear(_ctx: &mut Ctx, mask: types::GLbitfield) -> () {
            let _res = unsafe { gl::Clear(mask) };
            _res
        }
        pub fn _glClearBufferData(
            _ctx: &mut Ctx,
            target: types::GLenum,
            internalformat: types::GLenum,
            format: types::GLenum,
            type_: types::GLenum,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::ClearBufferData(target, internalformat, format, type_, data) };
            _res
        }
        pub fn _glClearBufferSubData(
            _ctx: &mut Ctx,
            target: types::GLenum,
            internalformat: types::GLenum,
            offset: u32,
            size: u32,
            format: types::GLenum,
            type_: types::GLenum,
            data: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;

            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::ClearBufferSubData(target, internalformat, offset, size, format, type_, data)
            };
            _res
        }
        pub fn _glClearBufferfi(
            _ctx: &mut Ctx,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            depth: f64,
            stencil: types::GLint,
        ) -> () {
            let depth: types::GLfloat = depth as types::GLfloat;

            let _res = unsafe { gl::ClearBufferfi(buffer, drawbuffer, depth, stencil) };
            _res
        }
        pub fn _glClearBufferfv(
            _ctx: &mut Ctx,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::ClearBufferfv(buffer, drawbuffer, value) };
            _res
        }
        pub fn _glClearBufferiv(
            _ctx: &mut Ctx,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::ClearBufferiv(buffer, drawbuffer, value) };
            _res
        }
        pub fn _glClearBufferuiv(
            _ctx: &mut Ctx,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::ClearBufferuiv(buffer, drawbuffer, value) };
            _res
        }
        pub fn _glClearColor(_ctx: &mut Ctx, red: f64, green: f64, blue: f64, alpha: f64) -> () {
            let red: types::GLfloat = red as types::GLfloat;
            let green: types::GLfloat = green as types::GLfloat;
            let blue: types::GLfloat = blue as types::GLfloat;
            let alpha: types::GLfloat = alpha as types::GLfloat;
            let _res = unsafe { gl::ClearColor(red, green, blue, alpha) };
            _res
        }
        pub fn _glClearDepth(_ctx: &mut Ctx, depth: types::GLdouble) -> () {
            let _res = unsafe { gl::ClearDepth(depth) };
            _res
        }
        pub fn _glClearDepthf(_ctx: &mut Ctx, d: f64) -> () {
            let d: types::GLfloat = d as types::GLfloat;
            let _res = unsafe { gl::ClearDepthf(d) };
            _res
        }
        pub fn _glClearNamedBufferData(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            internalformat: types::GLenum,
            format: types::GLenum,
            type_: types::GLenum,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res =
                unsafe { gl::ClearNamedBufferData(buffer, internalformat, format, type_, data) };
            _res
        }
        pub fn _glClearNamedBufferSubData(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            internalformat: types::GLenum,
            offset: u32,
            size: u32,
            format: types::GLenum,
            type_: types::GLenum,
            data: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;

            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::ClearNamedBufferSubData(
                    buffer,
                    internalformat,
                    offset,
                    size,
                    format,
                    type_,
                    data,
                )
            };
            _res
        }
        pub fn _glClearNamedFramebufferfi(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            depth: f64,
            stencil: types::GLint,
        ) -> () {
            let depth: types::GLfloat = depth as types::GLfloat;

            let _res = unsafe {
                gl::ClearNamedFramebufferfi(framebuffer, buffer, drawbuffer, depth, stencil)
            };
            _res
        }
        pub fn _glClearNamedFramebufferfv(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res =
                unsafe { gl::ClearNamedFramebufferfv(framebuffer, buffer, drawbuffer, value) };
            _res
        }
        pub fn _glClearNamedFramebufferiv(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res =
                unsafe { gl::ClearNamedFramebufferiv(framebuffer, buffer, drawbuffer, value) };
            _res
        }
        pub fn _glClearNamedFramebufferuiv(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            buffer: types::GLenum,
            drawbuffer: types::GLint,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res =
                unsafe { gl::ClearNamedFramebufferuiv(framebuffer, buffer, drawbuffer, value) };
            _res
        }
        pub fn _glClearStencil(_ctx: &mut Ctx, s: types::GLint) -> () {
            let _res = unsafe { gl::ClearStencil(s) };
            _res
        }
        pub fn _glClearTexImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            format: types::GLenum,
            type_: types::GLenum,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::ClearTexImage(texture, level, format, type_, data) };
            _res
        }
        pub fn _glClearTexSubImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::ClearTexSubImage(
                    texture, level, xoffset, yoffset, zoffset, width, height, depth, format, type_,
                    data,
                )
            };
            _res
        }
        pub fn _glClientWaitSync(
            _ctx: &mut Ctx,
            sync: u32,
            flags: types::GLbitfield,
            timeout: types::GLuint64,
        ) -> types::GLenum {
            let sync: types::GLsync =
                emscripten_memory_pointer!(_ctx.memory(0), sync) as types::GLsync;

            let _res = unsafe { gl::ClientWaitSync(sync, flags, timeout) };
            _res
        }
        pub fn _glClipControl(_ctx: &mut Ctx, origin: types::GLenum, depth: types::GLenum) -> () {
            let _res = unsafe { gl::ClipControl(origin, depth) };
            _res
        }
        pub fn _glColorMask(
            _ctx: &mut Ctx,
            red: types::GLboolean,
            green: types::GLboolean,
            blue: types::GLboolean,
            alpha: types::GLboolean,
        ) -> () {
            let _res = unsafe { gl::ColorMask(red, green, blue, alpha) };
            _res
        }
        pub fn _glColorMaski(
            _ctx: &mut Ctx,
            index: types::GLuint,
            r: types::GLboolean,
            g: types::GLboolean,
            b: types::GLboolean,
            a: types::GLboolean,
        ) -> () {
            let _res = unsafe { gl::ColorMaski(index, r, g, b, a) };
            _res
        }
        pub fn _glColorP3ui(_ctx: &mut Ctx, type_: types::GLenum, color: types::GLuint) -> () {
            let _res = unsafe { gl::ColorP3ui(type_, color) };
            _res
        }
        pub fn _glColorP3uiv(_ctx: &mut Ctx, type_: types::GLenum, color: u32) -> () {
            let color: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), color) as *const types::GLuint;
            let _res = unsafe { gl::ColorP3uiv(type_, color) };
            _res
        }
        pub fn _glColorP4ui(_ctx: &mut Ctx, type_: types::GLenum, color: types::GLuint) -> () {
            let _res = unsafe { gl::ColorP4ui(type_, color) };
            _res
        }
        pub fn _glColorP4uiv(_ctx: &mut Ctx, type_: types::GLenum, color: u32) -> () {
            let color: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), color) as *const types::GLuint;
            let _res = unsafe { gl::ColorP4uiv(type_, color) };
            _res
        }
        pub fn _glCompileShader(_ctx: &mut Ctx, shader: types::GLuint) -> () {
            let _res = unsafe { gl::CompileShader(shader) };
            _res
        }
        pub fn _glCompressedTexImage1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLenum,
            width: types::GLsizei,
            border: types::GLint,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTexImage1D(
                    target,
                    level,
                    internalformat,
                    width,
                    border,
                    imageSize,
                    data,
                )
            };
            _res
        }
        pub fn _glCompressedTexImage2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            border: types::GLint,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTexImage2D(
                    target,
                    level,
                    internalformat,
                    width,
                    height,
                    border,
                    imageSize,
                    data,
                )
            };
            _res
        }
        pub fn _glCompressedTexImage3D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            border: types::GLint,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTexImage3D(
                    target,
                    level,
                    internalformat,
                    width,
                    height,
                    depth,
                    border,
                    imageSize,
                    data,
                )
            };
            _res
        }
        pub fn _glCompressedTexSubImage1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            width: types::GLsizei,
            format: types::GLenum,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTexSubImage1D(target, level, xoffset, width, format, imageSize, data)
            };
            _res
        }
        pub fn _glCompressedTexSubImage2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            format: types::GLenum,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTexSubImage2D(
                    target, level, xoffset, yoffset, width, height, format, imageSize, data,
                )
            };
            _res
        }
        pub fn _glCompressedTexSubImage3D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            format: types::GLenum,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTexSubImage3D(
                    target, level, xoffset, yoffset, zoffset, width, height, depth, format,
                    imageSize, data,
                )
            };
            _res
        }
        pub fn _glCompressedTextureSubImage1D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            width: types::GLsizei,
            format: types::GLenum,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTextureSubImage1D(
                    texture, level, xoffset, width, format, imageSize, data,
                )
            };
            _res
        }
        pub fn _glCompressedTextureSubImage2D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            format: types::GLenum,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTextureSubImage2D(
                    texture, level, xoffset, yoffset, width, height, format, imageSize, data,
                )
            };
            _res
        }
        pub fn _glCompressedTextureSubImage3D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            format: types::GLenum,
            imageSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::CompressedTextureSubImage3D(
                    texture, level, xoffset, yoffset, zoffset, width, height, depth, format,
                    imageSize, data,
                )
            };
            _res
        }
        pub fn _glCopyBufferSubData(
            _ctx: &mut Ctx,
            readTarget: types::GLenum,
            writeTarget: types::GLenum,
            readOffset: u32,
            writeOffset: u32,
            size: u32,
        ) -> () {
            let readOffset: types::GLintptr = readOffset as types::GLintptr;
            let writeOffset: types::GLintptr = writeOffset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let _res = unsafe {
                gl::CopyBufferSubData(readTarget, writeTarget, readOffset, writeOffset, size)
            };
            _res
        }
        pub fn _glCopyImageSubData(
            _ctx: &mut Ctx,
            srcName: types::GLuint,
            srcTarget: types::GLenum,
            srcLevel: types::GLint,
            srcX: types::GLint,
            srcY: types::GLint,
            srcZ: types::GLint,
            dstName: types::GLuint,
            dstTarget: types::GLenum,
            dstLevel: types::GLint,
            dstX: types::GLint,
            dstY: types::GLint,
            dstZ: types::GLint,
            srcWidth: types::GLsizei,
            srcHeight: types::GLsizei,
            srcDepth: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::CopyImageSubData(
                    srcName, srcTarget, srcLevel, srcX, srcY, srcZ, dstName, dstTarget, dstLevel,
                    dstX, dstY, dstZ, srcWidth, srcHeight, srcDepth,
                )
            };
            _res
        }
        pub fn _glCopyNamedBufferSubData(
            _ctx: &mut Ctx,
            readBuffer: types::GLuint,
            writeBuffer: types::GLuint,
            readOffset: u32,
            writeOffset: u32,
            size: u32,
        ) -> () {
            let readOffset: types::GLintptr = readOffset as types::GLintptr;
            let writeOffset: types::GLintptr = writeOffset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let _res = unsafe {
                gl::CopyNamedBufferSubData(readBuffer, writeBuffer, readOffset, writeOffset, size)
            };
            _res
        }
        pub fn _glCopyTexImage1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLenum,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            border: types::GLint,
        ) -> () {
            let _res =
                unsafe { gl::CopyTexImage1D(target, level, internalformat, x, y, width, border) };
            _res
        }
        pub fn _glCopyTexImage2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLenum,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            border: types::GLint,
        ) -> () {
            let _res = unsafe {
                gl::CopyTexImage2D(target, level, internalformat, x, y, width, height, border)
            };
            _res
        }
        pub fn _glCopyTexSubImage1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::CopyTexSubImage1D(target, level, xoffset, x, y, width) };
            _res
        }
        pub fn _glCopyTexSubImage2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::CopyTexSubImage2D(target, level, xoffset, yoffset, x, y, width, height)
            };
            _res
        }
        pub fn _glCopyTexSubImage3D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::CopyTexSubImage3D(
                    target, level, xoffset, yoffset, zoffset, x, y, width, height,
                )
            };
            _res
        }
        pub fn _glCopyTextureSubImage1D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::CopyTextureSubImage1D(texture, level, xoffset, x, y, width) };
            _res
        }
        pub fn _glCopyTextureSubImage2D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::CopyTextureSubImage2D(texture, level, xoffset, yoffset, x, y, width, height)
            };
            _res
        }
        pub fn _glCopyTextureSubImage3D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::CopyTextureSubImage3D(
                    texture, level, xoffset, yoffset, zoffset, x, y, width, height,
                )
            };
            _res
        }
        pub fn _glCreateBuffers(_ctx: &mut Ctx, n: types::GLsizei, buffers: u32) -> () {
            let buffers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), buffers) as *mut types::GLuint;
            let _res = unsafe { gl::CreateBuffers(n, buffers) };
            _res
        }
        pub fn _glCreateFramebuffers(_ctx: &mut Ctx, n: types::GLsizei, framebuffers: u32) -> () {
            let framebuffers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), framebuffers) as *mut types::GLuint;
            let _res = unsafe { gl::CreateFramebuffers(n, framebuffers) };
            _res
        }
        pub fn _glCreateProgram(_ctx: &mut Ctx) -> types::GLuint {
            let _res = unsafe { gl::CreateProgram() };
            _res
        }
        pub fn _glCreateProgramPipelines(_ctx: &mut Ctx, n: types::GLsizei, pipelines: u32) -> () {
            let pipelines: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), pipelines) as *mut types::GLuint;
            let _res = unsafe { gl::CreateProgramPipelines(n, pipelines) };
            _res
        }
        pub fn _glCreateQueries(
            _ctx: &mut Ctx,
            target: types::GLenum,
            n: types::GLsizei,
            ids: u32,
        ) -> () {
            let ids: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *mut types::GLuint;
            let _res = unsafe { gl::CreateQueries(target, n, ids) };
            _res
        }
        pub fn _glCreateRenderbuffers(_ctx: &mut Ctx, n: types::GLsizei, renderbuffers: u32) -> () {
            let renderbuffers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), renderbuffers) as *mut types::GLuint;
            let _res = unsafe { gl::CreateRenderbuffers(n, renderbuffers) };
            _res
        }
        pub fn _glCreateSamplers(_ctx: &mut Ctx, n: types::GLsizei, samplers: u32) -> () {
            let samplers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), samplers) as *mut types::GLuint;
            let _res = unsafe { gl::CreateSamplers(n, samplers) };
            _res
        }
        pub fn _glCreateShader(_ctx: &mut Ctx, type_: types::GLenum) -> types::GLuint {
            let _res = unsafe { gl::CreateShader(type_) };
            _res
        }
        pub fn _glCreateShaderProgramv(
            _ctx: &mut Ctx,
            type_: types::GLenum,
            count: types::GLsizei,
            strings: u32,
        ) -> types::GLuint {
            let strings: *const *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), strings) as *const *const types::GLchar;
            let _res = unsafe { gl::CreateShaderProgramv(type_, count, strings) };
            _res
        }
        pub fn _glCreateTextures(
            _ctx: &mut Ctx,
            target: types::GLenum,
            n: types::GLsizei,
            textures: u32,
        ) -> () {
            let textures: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), textures) as *mut types::GLuint;
            let _res = unsafe { gl::CreateTextures(target, n, textures) };
            _res
        }
        pub fn _glCreateTransformFeedbacks(_ctx: &mut Ctx, n: types::GLsizei, ids: u32) -> () {
            let ids: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *mut types::GLuint;
            let _res = unsafe { gl::CreateTransformFeedbacks(n, ids) };
            _res
        }
        pub fn _glCreateVertexArrays(_ctx: &mut Ctx, n: types::GLsizei, arrays: u32) -> () {
            let arrays: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), arrays) as *mut types::GLuint;
            let _res = unsafe { gl::CreateVertexArrays(n, arrays) };
            _res
        }
        pub fn _glCullFace(_ctx: &mut Ctx, mode: types::GLenum) -> () {
            let _res = unsafe { gl::CullFace(mode) };
            _res
        }
        pub fn _glDebugMessageCallback(
            _ctx: &mut Ctx,
            callback: types::GLDEBUGPROC,
            userParam: u32,
        ) -> () {
            let userParam: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), userParam)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::DebugMessageCallback(callback, userParam) };
            _res
        }
        pub fn _glDebugMessageControl(
            _ctx: &mut Ctx,
            source: types::GLenum,
            type_: types::GLenum,
            severity: types::GLenum,
            count: types::GLsizei,
            ids: u32,
            enabled: types::GLboolean,
        ) -> () {
            let ids: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *const types::GLuint;

            let _res =
                unsafe { gl::DebugMessageControl(source, type_, severity, count, ids, enabled) };
            _res
        }
        pub fn _glDebugMessageInsert(
            _ctx: &mut Ctx,
            source: types::GLenum,
            type_: types::GLenum,
            id: types::GLuint,
            severity: types::GLenum,
            length: types::GLsizei,
            buf: u32,
        ) -> () {
            let buf: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), buf) as *const types::GLchar;
            let _res = unsafe { gl::DebugMessageInsert(source, type_, id, severity, length, buf) };
            _res
        }
        pub fn _glDeleteBuffers(_ctx: &mut Ctx, n: types::GLsizei, buffers: u32) -> () {
            let buffers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), buffers) as *const types::GLuint;
            let _res = unsafe { gl::DeleteBuffers(n, buffers) };
            _res
        }
        pub fn _glDeleteFramebuffers(_ctx: &mut Ctx, n: types::GLsizei, framebuffers: u32) -> () {
            let framebuffers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), framebuffers) as *const types::GLuint;
            let _res = unsafe { gl::DeleteFramebuffers(n, framebuffers) };
            _res
        }
        pub fn _glDeleteProgram(_ctx: &mut Ctx, program: types::GLuint) -> () {
            let _res = unsafe { gl::DeleteProgram(program) };
            _res
        }
        pub fn _glDeleteProgramPipelines(_ctx: &mut Ctx, n: types::GLsizei, pipelines: u32) -> () {
            let pipelines: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), pipelines) as *const types::GLuint;
            let _res = unsafe { gl::DeleteProgramPipelines(n, pipelines) };
            _res
        }
        pub fn _glDeleteQueries(_ctx: &mut Ctx, n: types::GLsizei, ids: u32) -> () {
            let ids: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *const types::GLuint;
            let _res = unsafe { gl::DeleteQueries(n, ids) };
            _res
        }
        pub fn _glDeleteRenderbuffers(_ctx: &mut Ctx, n: types::GLsizei, renderbuffers: u32) -> () {
            let renderbuffers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), renderbuffers) as *const types::GLuint;
            let _res = unsafe { gl::DeleteRenderbuffers(n, renderbuffers) };
            _res
        }
        pub fn _glDeleteSamplers(_ctx: &mut Ctx, count: types::GLsizei, samplers: u32) -> () {
            let samplers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), samplers) as *const types::GLuint;
            let _res = unsafe { gl::DeleteSamplers(count, samplers) };
            _res
        }
        pub fn _glDeleteShader(_ctx: &mut Ctx, shader: types::GLuint) -> () {
            let _res = unsafe { gl::DeleteShader(shader) };
            _res
        }
        pub fn _glDeleteSync(_ctx: &mut Ctx, sync: u32) -> () {
            let sync: types::GLsync =
                emscripten_memory_pointer!(_ctx.memory(0), sync) as types::GLsync;
            let _res = unsafe { gl::DeleteSync(sync) };
            _res
        }
        pub fn _glDeleteTextures(_ctx: &mut Ctx, n: types::GLsizei, textures: u32) -> () {
            let textures: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), textures) as *const types::GLuint;
            let _res = unsafe { gl::DeleteTextures(n, textures) };
            _res
        }
        pub fn _glDeleteTransformFeedbacks(_ctx: &mut Ctx, n: types::GLsizei, ids: u32) -> () {
            let ids: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *const types::GLuint;
            let _res = unsafe { gl::DeleteTransformFeedbacks(n, ids) };
            _res
        }
        pub fn _glDeleteVertexArrays(_ctx: &mut Ctx, n: types::GLsizei, arrays: u32) -> () {
            let arrays: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), arrays) as *const types::GLuint;
            let _res = unsafe { gl::DeleteVertexArrays(n, arrays) };
            _res
        }
        pub fn _glDepthFunc(_ctx: &mut Ctx, func: types::GLenum) -> () {
            let _res = unsafe { gl::DepthFunc(func) };
            _res
        }
        pub fn _glDepthMask(_ctx: &mut Ctx, flag: types::GLboolean) -> () {
            let _res = unsafe { gl::DepthMask(flag) };
            _res
        }
        pub fn _glDepthRange(_ctx: &mut Ctx, n: types::GLdouble, f: types::GLdouble) -> () {
            let _res = unsafe { gl::DepthRange(n, f) };
            _res
        }
        pub fn _glDepthRangeArrayv(
            _ctx: &mut Ctx,
            first: types::GLuint,
            count: types::GLsizei,
            v: u32,
        ) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::DepthRangeArrayv(first, count, v) };
            _res
        }
        pub fn _glDepthRangeIndexed(
            _ctx: &mut Ctx,
            index: types::GLuint,
            n: types::GLdouble,
            f: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::DepthRangeIndexed(index, n, f) };
            _res
        }
        pub fn _glDepthRangef(_ctx: &mut Ctx, n: f64, f: f64) -> () {
            let n: types::GLfloat = n as types::GLfloat;
            let f: types::GLfloat = f as types::GLfloat;
            let _res = unsafe { gl::DepthRangef(n, f) };
            _res
        }
        pub fn _glDetachShader(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shader: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::DetachShader(program, shader) };
            _res
        }
        pub fn _glDisable(_ctx: &mut Ctx, cap: types::GLenum) -> () {
            let _res = unsafe { gl::Disable(cap) };
            _res
        }
        pub fn _glDisableVertexArrayAttrib(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            index: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::DisableVertexArrayAttrib(vaobj, index) };
            _res
        }
        pub fn _glDisableVertexAttribArray(_ctx: &mut Ctx, index: types::GLuint) -> () {
            let _res = unsafe { gl::DisableVertexAttribArray(index) };
            _res
        }
        pub fn _glDisablei(_ctx: &mut Ctx, target: types::GLenum, index: types::GLuint) -> () {
            let _res = unsafe { gl::Disablei(target, index) };
            _res
        }
        pub fn _glDispatchCompute(
            _ctx: &mut Ctx,
            num_groups_x: types::GLuint,
            num_groups_y: types::GLuint,
            num_groups_z: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::DispatchCompute(num_groups_x, num_groups_y, num_groups_z) };
            _res
        }
        pub fn _glDispatchComputeIndirect(_ctx: &mut Ctx, indirect: u32) -> () {
            let indirect: types::GLintptr = indirect as types::GLintptr;
            let _res = unsafe { gl::DispatchComputeIndirect(indirect) };
            _res
        }
        pub fn _glDrawArrays(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            first: types::GLint,
            count: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::DrawArrays(mode, first, count) };
            _res
        }
        pub fn _glDrawArraysIndirect(_ctx: &mut Ctx, mode: types::GLenum, indirect: u32) -> () {
            let indirect: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indirect)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::DrawArraysIndirect(mode, indirect) };
            _res
        }
        pub fn _glDrawArraysInstanced(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            first: types::GLint,
            count: types::GLsizei,
            instancecount: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::DrawArraysInstanced(mode, first, count, instancecount) };
            _res
        }
        pub fn _glDrawArraysInstancedBaseInstance(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            first: types::GLint,
            count: types::GLsizei,
            instancecount: types::GLsizei,
            baseinstance: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::DrawArraysInstancedBaseInstance(mode, first, count, instancecount, baseinstance)
            };
            _res
        }
        pub fn _glDrawBuffer(_ctx: &mut Ctx, buf: types::GLenum) -> () {
            let _res = unsafe { gl::DrawBuffer(buf) };
            _res
        }
        pub fn _glDrawBuffers(_ctx: &mut Ctx, n: types::GLsizei, bufs: u32) -> () {
            let bufs: *const types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), bufs) as *const types::GLenum;
            let _res = unsafe { gl::DrawBuffers(n, bufs) };
            _res
        }
        pub fn _glDrawElements(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::DrawElements(mode, count, type_, indices) };
            _res
        }
        pub fn _glDrawElementsBaseVertex(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
            basevertex: types::GLint,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;

            let _res =
                unsafe { gl::DrawElementsBaseVertex(mode, count, type_, indices, basevertex) };
            _res
        }
        pub fn _glDrawElementsIndirect(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            type_: types::GLenum,
            indirect: u32,
        ) -> () {
            let indirect: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indirect)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::DrawElementsIndirect(mode, type_, indirect) };
            _res
        }
        pub fn _glDrawElementsInstanced(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
            instancecount: types::GLsizei,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;

            let _res =
                unsafe { gl::DrawElementsInstanced(mode, count, type_, indices, instancecount) };
            _res
        }
        pub fn _glDrawElementsInstancedBaseInstance(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
            instancecount: types::GLsizei,
            baseinstance: types::GLuint,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe {
                gl::DrawElementsInstancedBaseInstance(
                    mode,
                    count,
                    type_,
                    indices,
                    instancecount,
                    baseinstance,
                )
            };
            _res
        }
        pub fn _glDrawElementsInstancedBaseVertex(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
            instancecount: types::GLsizei,
            basevertex: types::GLint,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe {
                gl::DrawElementsInstancedBaseVertex(
                    mode,
                    count,
                    type_,
                    indices,
                    instancecount,
                    basevertex,
                )
            };
            _res
        }
        pub fn _glDrawElementsInstancedBaseVertexBaseInstance(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
            instancecount: types::GLsizei,
            basevertex: types::GLint,
            baseinstance: types::GLuint,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe {
                gl::DrawElementsInstancedBaseVertexBaseInstance(
                    mode,
                    count,
                    type_,
                    indices,
                    instancecount,
                    basevertex,
                    baseinstance,
                )
            };
            _res
        }
        pub fn _glDrawRangeElements(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            start: types::GLuint,
            end: types::GLuint,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::DrawRangeElements(mode, start, end, count, type_, indices) };
            _res
        }
        pub fn _glDrawRangeElementsBaseVertex(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            start: types::GLuint,
            end: types::GLuint,
            count: types::GLsizei,
            type_: types::GLenum,
            indices: u32,
            basevertex: types::GLint,
        ) -> () {
            let indices: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe {
                gl::DrawRangeElementsBaseVertex(mode, start, end, count, type_, indices, basevertex)
            };
            _res
        }
        pub fn _glDrawTransformFeedback(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            id: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::DrawTransformFeedback(mode, id) };
            _res
        }
        pub fn _glDrawTransformFeedbackInstanced(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            id: types::GLuint,
            instancecount: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::DrawTransformFeedbackInstanced(mode, id, instancecount) };
            _res
        }
        pub fn _glDrawTransformFeedbackStream(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            id: types::GLuint,
            stream: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::DrawTransformFeedbackStream(mode, id, stream) };
            _res
        }
        pub fn _glDrawTransformFeedbackStreamInstanced(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            id: types::GLuint,
            stream: types::GLuint,
            instancecount: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::DrawTransformFeedbackStreamInstanced(mode, id, stream, instancecount)
            };
            _res
        }
        pub fn _glEnable(_ctx: &mut Ctx, cap: types::GLenum) -> () {
            let _res = unsafe { gl::Enable(cap) };
            _res
        }
        pub fn _glEnableVertexArrayAttrib(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            index: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::EnableVertexArrayAttrib(vaobj, index) };
            _res
        }
        pub fn _glEnableVertexAttribArray(_ctx: &mut Ctx, index: types::GLuint) -> () {
            let _res = unsafe { gl::EnableVertexAttribArray(index) };
            _res
        }
        pub fn _glEnablei(_ctx: &mut Ctx, target: types::GLenum, index: types::GLuint) -> () {
            let _res = unsafe { gl::Enablei(target, index) };
            _res
        }
        pub fn _glEndConditionalRender(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::EndConditionalRender() };
            _res
        }
        pub fn _glEndQuery(_ctx: &mut Ctx, target: types::GLenum) -> () {
            let _res = unsafe { gl::EndQuery(target) };
            _res
        }
        pub fn _glEndQueryIndexed(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::EndQueryIndexed(target, index) };
            _res
        }
        pub fn _glEndTransformFeedback(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::EndTransformFeedback() };
            _res
        }
        pub fn _glFenceSync(
            _ctx: &mut Ctx,
            condition: types::GLenum,
            flags: types::GLbitfield,
        ) -> u32 {
            let _res = unsafe { gl::FenceSync(condition, flags) };
            0
        }
        pub fn _glFinish(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::Finish() };
            _res
        }
        pub fn _glFlush(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::Flush() };
            _res
        }
        pub fn _glFlushMappedBufferRange(
            _ctx: &mut Ctx,
            target: types::GLenum,
            offset: u32,
            length: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let length: types::GLsizeiptr = length as types::GLsizeiptr;
            let _res = unsafe { gl::FlushMappedBufferRange(target, offset, length) };
            _res
        }
        pub fn _glFlushMappedNamedBufferRange(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            offset: u32,
            length: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let length: types::GLsizeiptr = length as types::GLsizeiptr;
            let _res = unsafe { gl::FlushMappedNamedBufferRange(buffer, offset, length) };
            _res
        }
        pub fn _glFramebufferParameteri(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            param: types::GLint,
        ) -> () {
            let _res = unsafe { gl::FramebufferParameteri(target, pname, param) };
            _res
        }
        pub fn _glFramebufferRenderbuffer(
            _ctx: &mut Ctx,
            target: types::GLenum,
            attachment: types::GLenum,
            renderbuffertarget: types::GLenum,
            renderbuffer: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::FramebufferRenderbuffer(target, attachment, renderbuffertarget, renderbuffer)
            };
            _res
        }
        pub fn _glFramebufferTexture(
            _ctx: &mut Ctx,
            target: types::GLenum,
            attachment: types::GLenum,
            texture: types::GLuint,
            level: types::GLint,
        ) -> () {
            let _res = unsafe { gl::FramebufferTexture(target, attachment, texture, level) };
            _res
        }
        pub fn _glFramebufferTexture1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            attachment: types::GLenum,
            textarget: types::GLenum,
            texture: types::GLuint,
            level: types::GLint,
        ) -> () {
            let _res =
                unsafe { gl::FramebufferTexture1D(target, attachment, textarget, texture, level) };
            _res
        }
        pub fn _glFramebufferTexture2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            attachment: types::GLenum,
            textarget: types::GLenum,
            texture: types::GLuint,
            level: types::GLint,
        ) -> () {
            let _res =
                unsafe { gl::FramebufferTexture2D(target, attachment, textarget, texture, level) };
            _res
        }
        pub fn _glFramebufferTexture3D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            attachment: types::GLenum,
            textarget: types::GLenum,
            texture: types::GLuint,
            level: types::GLint,
            zoffset: types::GLint,
        ) -> () {
            let _res = unsafe {
                gl::FramebufferTexture3D(target, attachment, textarget, texture, level, zoffset)
            };
            _res
        }
        pub fn _glFramebufferTextureLayer(
            _ctx: &mut Ctx,
            target: types::GLenum,
            attachment: types::GLenum,
            texture: types::GLuint,
            level: types::GLint,
            layer: types::GLint,
        ) -> () {
            let _res =
                unsafe { gl::FramebufferTextureLayer(target, attachment, texture, level, layer) };
            _res
        }
        pub fn _glFrontFace(_ctx: &mut Ctx, mode: types::GLenum) -> () {
            let _res = unsafe { gl::FrontFace(mode) };
            _res
        }
        pub fn _glGenBuffers(_ctx: &mut Ctx, n: types::GLsizei, buffers: u32) -> () {
            let buffers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), buffers) as *mut types::GLuint;
            let _res = unsafe { gl::GenBuffers(n, buffers) };
            _res
        }
        pub fn _glGenFramebuffers(_ctx: &mut Ctx, n: types::GLsizei, framebuffers: u32) -> () {
            let framebuffers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), framebuffers) as *mut types::GLuint;
            let _res = unsafe { gl::GenFramebuffers(n, framebuffers) };
            _res
        }
        pub fn _glGenProgramPipelines(_ctx: &mut Ctx, n: types::GLsizei, pipelines: u32) -> () {
            let pipelines: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), pipelines) as *mut types::GLuint;
            let _res = unsafe { gl::GenProgramPipelines(n, pipelines) };
            _res
        }
        pub fn _glGenQueries(_ctx: &mut Ctx, n: types::GLsizei, ids: u32) -> () {
            let ids: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *mut types::GLuint;
            let _res = unsafe { gl::GenQueries(n, ids) };
            _res
        }
        pub fn _glGenRenderbuffers(_ctx: &mut Ctx, n: types::GLsizei, renderbuffers: u32) -> () {
            let renderbuffers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), renderbuffers) as *mut types::GLuint;
            let _res = unsafe { gl::GenRenderbuffers(n, renderbuffers) };
            _res
        }
        pub fn _glGenSamplers(_ctx: &mut Ctx, count: types::GLsizei, samplers: u32) -> () {
            let samplers: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), samplers) as *mut types::GLuint;
            let _res = unsafe { gl::GenSamplers(count, samplers) };
            _res
        }
        pub fn _glGenTextures(_ctx: &mut Ctx, n: types::GLsizei, textures: u32) -> () {
            let textures: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), textures) as *mut types::GLuint;
            let _res = unsafe { gl::GenTextures(n, textures) };
            _res
        }
        pub fn _glGenTransformFeedbacks(_ctx: &mut Ctx, n: types::GLsizei, ids: u32) -> () {
            let ids: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *mut types::GLuint;
            let _res = unsafe { gl::GenTransformFeedbacks(n, ids) };
            _res
        }
        pub fn _glGenVertexArrays(_ctx: &mut Ctx, n: types::GLsizei, arrays: u32) -> () {
            let arrays: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), arrays) as *mut types::GLuint;
            let _res = unsafe { gl::GenVertexArrays(n, arrays) };
            _res
        }
        pub fn _glGenerateMipmap(_ctx: &mut Ctx, target: types::GLenum) -> () {
            let _res = unsafe { gl::GenerateMipmap(target) };
            _res
        }
        pub fn _glGenerateTextureMipmap(_ctx: &mut Ctx, texture: types::GLuint) -> () {
            let _res = unsafe { gl::GenerateTextureMipmap(texture) };
            _res
        }
        pub fn _glGetActiveAtomicCounterBufferiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            bufferIndex: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res =
                unsafe { gl::GetActiveAtomicCounterBufferiv(program, bufferIndex, pname, params) };
            _res
        }
        pub fn _glGetActiveAttrib(
            _ctx: &mut Ctx,
            program: types::GLuint,
            index: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            size: u32,
            type_: u32,
            name: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let size: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), size) as *mut types::GLint;
            let type_: *mut types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), type_) as *mut types::GLenum;
            let name: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *mut types::GLchar;
            let _res =
                unsafe { gl::GetActiveAttrib(program, index, bufSize, length, size, type_, name) };
            _res
        }
        pub fn _glGetActiveSubroutineName(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shadertype: types::GLenum,
            index: types::GLuint,
            bufsize: types::GLsizei,
            length: u32,
            name: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let name: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *mut types::GLchar;
            let _res = unsafe {
                gl::GetActiveSubroutineName(program, shadertype, index, bufsize, length, name)
            };
            _res
        }
        pub fn _glGetActiveSubroutineUniformName(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shadertype: types::GLenum,
            index: types::GLuint,
            bufsize: types::GLsizei,
            length: u32,
            name: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let name: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *mut types::GLchar;
            let _res = unsafe {
                gl::GetActiveSubroutineUniformName(
                    program, shadertype, index, bufsize, length, name,
                )
            };
            _res
        }
        pub fn _glGetActiveSubroutineUniformiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shadertype: types::GLenum,
            index: types::GLuint,
            pname: types::GLenum,
            values: u32,
        ) -> () {
            let values: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), values) as *mut types::GLint;
            let _res = unsafe {
                gl::GetActiveSubroutineUniformiv(program, shadertype, index, pname, values)
            };
            _res
        }
        pub fn _glGetActiveUniform(
            _ctx: &mut Ctx,
            program: types::GLuint,
            index: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            size: u32,
            type_: u32,
            name: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let size: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), size) as *mut types::GLint;
            let type_: *mut types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), type_) as *mut types::GLenum;
            let name: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *mut types::GLchar;
            let _res =
                unsafe { gl::GetActiveUniform(program, index, bufSize, length, size, type_, name) };
            _res
        }
        pub fn _glGetActiveUniformBlockName(
            _ctx: &mut Ctx,
            program: types::GLuint,
            uniformBlockIndex: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            uniformBlockName: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let uniformBlockName: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), uniformBlockName) as *mut types::GLchar;
            let _res = unsafe {
                gl::GetActiveUniformBlockName(
                    program,
                    uniformBlockIndex,
                    bufSize,
                    length,
                    uniformBlockName,
                )
            };
            _res
        }
        pub fn _glGetActiveUniformBlockiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            uniformBlockIndex: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res =
                unsafe { gl::GetActiveUniformBlockiv(program, uniformBlockIndex, pname, params) };
            _res
        }
        pub fn _glGetActiveUniformName(
            _ctx: &mut Ctx,
            program: types::GLuint,
            uniformIndex: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            uniformName: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let uniformName: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), uniformName) as *mut types::GLchar;
            let _res = unsafe {
                gl::GetActiveUniformName(program, uniformIndex, bufSize, length, uniformName)
            };
            _res
        }
        pub fn _glGetActiveUniformsiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            uniformCount: types::GLsizei,
            uniformIndices: u32,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let uniformIndices: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), uniformIndices) as *const types::GLuint;

            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe {
                gl::GetActiveUniformsiv(program, uniformCount, uniformIndices, pname, params)
            };
            _res
        }
        pub fn _glGetAttachedShaders(
            _ctx: &mut Ctx,
            program: types::GLuint,
            maxCount: types::GLsizei,
            count: u32,
            shaders: u32,
        ) -> () {
            let count: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), count) as *mut types::GLsizei;
            let shaders: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), shaders) as *mut types::GLuint;
            let _res = unsafe { gl::GetAttachedShaders(program, maxCount, count, shaders) };
            _res
        }
        pub fn _glGetAttribLocation(
            _ctx: &mut Ctx,
            program: types::GLuint,
            name: u32,
        ) -> types::GLint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetAttribLocation(program, name) };
            _res
        }
        pub fn _glGetBooleani_v(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            data: u32,
        ) -> () {
            let data: *mut types::GLboolean =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLboolean;
            let _res = unsafe { gl::GetBooleani_v(target, index, data) };
            _res
        }
        pub fn _glGetBooleanv(_ctx: &mut Ctx, pname: types::GLenum, data: u32) -> () {
            let data: *mut types::GLboolean =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLboolean;
            let _res = unsafe { gl::GetBooleanv(pname, data) };
            _res
        }
        pub fn _glGetBufferParameteri64v(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint64;
            let _res = unsafe { gl::GetBufferParameteri64v(target, pname, params) };
            _res
        }
        pub fn _glGetBufferParameteriv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetBufferParameteriv(target, pname, params) };
            _res
        }
        pub fn _glGetBufferPointerv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), params)
                    as *const *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetBufferPointerv(target, pname, params) };
            _res
        }
        pub fn _glGetBufferSubData(
            _ctx: &mut Ctx,
            target: types::GLenum,
            offset: u32,
            size: u32,
            data: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetBufferSubData(target, offset, size, data) };
            _res
        }
        pub fn _glGetCompressedTexImage(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            img: u32,
        ) -> () {
            let img: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), img) as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetCompressedTexImage(target, level, img) };
            _res
        }
        pub fn _glGetCompressedTextureImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            bufSize: types::GLsizei,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetCompressedTextureImage(texture, level, bufSize, pixels) };
            _res
        }
        pub fn _glGetCompressedTextureSubImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            bufSize: types::GLsizei,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::GetCompressedTextureSubImage(
                    texture, level, xoffset, yoffset, zoffset, width, height, depth, bufSize,
                    pixels,
                )
            };
            _res
        }
        pub fn _glGetDebugMessageLog(
            _ctx: &mut Ctx,
            count: types::GLuint,
            bufSize: types::GLsizei,
            sources: u32,
            types: u32,
            ids: u32,
            severities: u32,
            lengths: u32,
            messageLog: u32,
        ) -> types::GLuint {
            let sources: *mut types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), sources) as *mut types::GLenum;
            let types: *mut types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), types) as *mut types::GLenum;
            let ids: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), ids) as *mut types::GLuint;
            let severities: *mut types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), severities) as *mut types::GLenum;
            let lengths: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), lengths) as *mut types::GLsizei;
            let messageLog: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), messageLog) as *mut types::GLchar;
            let _res = unsafe {
                gl::GetDebugMessageLog(
                    count, bufSize, sources, types, ids, severities, lengths, messageLog,
                )
            };
            _res
        }
        pub fn _glGetDoublei_v(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            data: u32,
        ) -> () {
            let data: *mut types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLdouble;
            let _res = unsafe { gl::GetDoublei_v(target, index, data) };
            _res
        }
        pub fn _glGetDoublev(_ctx: &mut Ctx, pname: types::GLenum, data: u32) -> () {
            let data: *mut types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLdouble;
            let _res = unsafe { gl::GetDoublev(pname, data) };
            _res
        }
        pub fn _glGetError(_ctx: &mut Ctx) -> types::GLenum {
            let _res = unsafe { gl::GetError() };
            _res
        }
        pub fn _glGetFloati_v(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            data: u32,
        ) -> () {
            let data: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLfloat;
            let _res = unsafe { gl::GetFloati_v(target, index, data) };
            _res
        }
        pub fn _glGetFloatv(_ctx: &mut Ctx, pname: types::GLenum, data: u32) -> () {
            let data: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLfloat;
            let _res = unsafe { gl::GetFloatv(pname, data) };
            _res
        }
        pub fn _glGetFragDataIndex(
            _ctx: &mut Ctx,
            program: types::GLuint,
            name: u32,
        ) -> types::GLint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetFragDataIndex(program, name) };
            _res
        }
        pub fn _glGetFragDataLocation(
            _ctx: &mut Ctx,
            program: types::GLuint,
            name: u32,
        ) -> types::GLint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetFragDataLocation(program, name) };
            _res
        }
        pub fn _glGetFramebufferAttachmentParameteriv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            attachment: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe {
                gl::GetFramebufferAttachmentParameteriv(target, attachment, pname, params)
            };
            _res
        }
        pub fn _glGetFramebufferParameteriv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetFramebufferParameteriv(target, pname, params) };
            _res
        }
        pub fn _glGetGraphicsResetStatus(_ctx: &mut Ctx) -> types::GLenum {
            let _res = unsafe { gl::GetGraphicsResetStatus() };
            _res
        }
        pub fn _glGetInteger64i_v(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            data: u32,
        ) -> () {
            let data: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLint64;
            let _res = unsafe { gl::GetInteger64i_v(target, index, data) };
            _res
        }
        pub fn _glGetInteger64v(_ctx: &mut Ctx, pname: types::GLenum, data: u32) -> () {
            let data: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLint64;
            let _res = unsafe { gl::GetInteger64v(pname, data) };
            _res
        }
        pub fn _glGetIntegeri_v(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            data: u32,
        ) -> () {
            let data: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLint;
            let _res = unsafe { gl::GetIntegeri_v(target, index, data) };
            _res
        }
        pub fn _glGetIntegerv(_ctx: &mut Ctx, pname: types::GLenum, data: u32) -> () {
            let data: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut types::GLint;
            let _res = unsafe { gl::GetIntegerv(pname, data) };
            _res
        }
        pub fn _glGetInternalformati64v(
            _ctx: &mut Ctx,
            target: types::GLenum,
            internalformat: types::GLenum,
            pname: types::GLenum,
            bufSize: types::GLsizei,
            params: u32,
        ) -> () {
            let params: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint64;
            let _res = unsafe {
                gl::GetInternalformati64v(target, internalformat, pname, bufSize, params)
            };
            _res
        }
        pub fn _glGetInternalformativ(
            _ctx: &mut Ctx,
            target: types::GLenum,
            internalformat: types::GLenum,
            pname: types::GLenum,
            bufSize: types::GLsizei,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res =
                unsafe { gl::GetInternalformativ(target, internalformat, pname, bufSize, params) };
            _res
        }
        pub fn _glGetMultisamplefv(
            _ctx: &mut Ctx,
            pname: types::GLenum,
            index: types::GLuint,
            val: u32,
        ) -> () {
            let val: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), val) as *mut types::GLfloat;
            let _res = unsafe { gl::GetMultisamplefv(pname, index, val) };
            _res
        }
        pub fn _glGetNamedBufferParameteri64v(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint64;
            let _res = unsafe { gl::GetNamedBufferParameteri64v(buffer, pname, params) };
            _res
        }
        pub fn _glGetNamedBufferParameteriv(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetNamedBufferParameteriv(buffer, pname, params) };
            _res
        }
        pub fn _glGetNamedBufferPointerv(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), params)
                    as *const *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetNamedBufferPointerv(buffer, pname, params) };
            _res
        }
        pub fn _glGetNamedBufferSubData(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            offset: u32,
            size: u32,
            data: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetNamedBufferSubData(buffer, offset, size, data) };
            _res
        }
        pub fn _glGetNamedFramebufferAttachmentParameteriv(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            attachment: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe {
                gl::GetNamedFramebufferAttachmentParameteriv(framebuffer, attachment, pname, params)
            };
            _res
        }
        pub fn _glGetNamedFramebufferParameteriv(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *mut types::GLint;
            let _res = unsafe { gl::GetNamedFramebufferParameteriv(framebuffer, pname, param) };
            _res
        }
        pub fn _glGetNamedRenderbufferParameteriv(
            _ctx: &mut Ctx,
            renderbuffer: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetNamedRenderbufferParameteriv(renderbuffer, pname, params) };
            _res
        }
        pub fn _glGetObjectLabel(
            _ctx: &mut Ctx,
            identifier: types::GLenum,
            name: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            label: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let label: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), label) as *mut types::GLchar;
            let _res = unsafe { gl::GetObjectLabel(identifier, name, bufSize, length, label) };
            _res
        }
        pub fn _glGetObjectPtrLabel(
            _ctx: &mut Ctx,
            ptr: u32,
            bufSize: types::GLsizei,
            length: u32,
            label: u32,
        ) -> () {
            let ptr: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), ptr) as *const __gl_imports::raw::c_void;

            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let label: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), label) as *mut types::GLchar;
            let _res = unsafe { gl::GetObjectPtrLabel(ptr, bufSize, length, label) };
            _res
        }
        pub fn _glGetPointerv(_ctx: &mut Ctx, pname: types::GLenum, params: u32) -> () {
            let params: *const *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), params)
                    as *const *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetPointerv(pname, params) };
            _res
        }
        pub fn _glGetProgramBinary(
            _ctx: &mut Ctx,
            program: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            binaryFormat: u32,
            binary: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let binaryFormat: *mut types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), binaryFormat) as *mut types::GLenum;
            let binary: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), binary)
                    as *mut __gl_imports::raw::c_void;
            let _res =
                unsafe { gl::GetProgramBinary(program, bufSize, length, binaryFormat, binary) };
            _res
        }
        pub fn _glGetProgramInfoLog(
            _ctx: &mut Ctx,
            program: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            infoLog: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let infoLog: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), infoLog) as *mut types::GLchar;
            let _res = unsafe { gl::GetProgramInfoLog(program, bufSize, length, infoLog) };
            _res
        }
        pub fn _glGetProgramInterfaceiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            programInterface: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res =
                unsafe { gl::GetProgramInterfaceiv(program, programInterface, pname, params) };
            _res
        }
        pub fn _glGetProgramPipelineInfoLog(
            _ctx: &mut Ctx,
            pipeline: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            infoLog: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let infoLog: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), infoLog) as *mut types::GLchar;
            let _res = unsafe { gl::GetProgramPipelineInfoLog(pipeline, bufSize, length, infoLog) };
            _res
        }
        pub fn _glGetProgramPipelineiv(
            _ctx: &mut Ctx,
            pipeline: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetProgramPipelineiv(pipeline, pname, params) };
            _res
        }
        pub fn _glGetProgramResourceIndex(
            _ctx: &mut Ctx,
            program: types::GLuint,
            programInterface: types::GLenum,
            name: u32,
        ) -> types::GLuint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetProgramResourceIndex(program, programInterface, name) };
            _res
        }
        pub fn _glGetProgramResourceLocation(
            _ctx: &mut Ctx,
            program: types::GLuint,
            programInterface: types::GLenum,
            name: u32,
        ) -> types::GLint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetProgramResourceLocation(program, programInterface, name) };
            _res
        }
        pub fn _glGetProgramResourceLocationIndex(
            _ctx: &mut Ctx,
            program: types::GLuint,
            programInterface: types::GLenum,
            name: u32,
        ) -> types::GLint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res =
                unsafe { gl::GetProgramResourceLocationIndex(program, programInterface, name) };
            _res
        }
        pub fn _glGetProgramResourceName(
            _ctx: &mut Ctx,
            program: types::GLuint,
            programInterface: types::GLenum,
            index: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            name: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let name: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *mut types::GLchar;
            let _res = unsafe {
                gl::GetProgramResourceName(program, programInterface, index, bufSize, length, name)
            };
            _res
        }
        pub fn _glGetProgramResourceiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            programInterface: types::GLenum,
            index: types::GLuint,
            propCount: types::GLsizei,
            props: u32,
            bufSize: types::GLsizei,
            length: u32,
            params: u32,
        ) -> () {
            let props: *const types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), props) as *const types::GLenum;

            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe {
                gl::GetProgramResourceiv(
                    program,
                    programInterface,
                    index,
                    propCount,
                    props,
                    bufSize,
                    length,
                    params,
                )
            };
            _res
        }
        pub fn _glGetProgramStageiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shadertype: types::GLenum,
            pname: types::GLenum,
            values: u32,
        ) -> () {
            let values: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), values) as *mut types::GLint;
            let _res = unsafe { gl::GetProgramStageiv(program, shadertype, pname, values) };
            _res
        }
        pub fn _glGetProgramiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetProgramiv(program, pname, params) };
            _res
        }
        pub fn _glGetQueryBufferObjecti64v(
            _ctx: &mut Ctx,
            id: types::GLuint,
            buffer: types::GLuint,
            pname: types::GLenum,
            offset: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let _res = unsafe { gl::GetQueryBufferObjecti64v(id, buffer, pname, offset) };
            _res
        }
        pub fn _glGetQueryBufferObjectiv(
            _ctx: &mut Ctx,
            id: types::GLuint,
            buffer: types::GLuint,
            pname: types::GLenum,
            offset: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let _res = unsafe { gl::GetQueryBufferObjectiv(id, buffer, pname, offset) };
            _res
        }
        pub fn _glGetQueryBufferObjectui64v(
            _ctx: &mut Ctx,
            id: types::GLuint,
            buffer: types::GLuint,
            pname: types::GLenum,
            offset: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let _res = unsafe { gl::GetQueryBufferObjectui64v(id, buffer, pname, offset) };
            _res
        }
        pub fn _glGetQueryBufferObjectuiv(
            _ctx: &mut Ctx,
            id: types::GLuint,
            buffer: types::GLuint,
            pname: types::GLenum,
            offset: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let _res = unsafe { gl::GetQueryBufferObjectuiv(id, buffer, pname, offset) };
            _res
        }
        pub fn _glGetQueryIndexediv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetQueryIndexediv(target, index, pname, params) };
            _res
        }
        pub fn _glGetQueryObjecti64v(
            _ctx: &mut Ctx,
            id: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint64;
            let _res = unsafe { gl::GetQueryObjecti64v(id, pname, params) };
            _res
        }
        pub fn _glGetQueryObjectiv(
            _ctx: &mut Ctx,
            id: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetQueryObjectiv(id, pname, params) };
            _res
        }
        pub fn _glGetQueryObjectui64v(
            _ctx: &mut Ctx,
            id: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint64 =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint64;
            let _res = unsafe { gl::GetQueryObjectui64v(id, pname, params) };
            _res
        }
        pub fn _glGetQueryObjectuiv(
            _ctx: &mut Ctx,
            id: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetQueryObjectuiv(id, pname, params) };
            _res
        }
        pub fn _glGetQueryiv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetQueryiv(target, pname, params) };
            _res
        }
        pub fn _glGetRenderbufferParameteriv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetRenderbufferParameteriv(target, pname, params) };
            _res
        }
        pub fn _glGetSamplerParameterIiv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetSamplerParameterIiv(sampler, pname, params) };
            _res
        }
        pub fn _glGetSamplerParameterIuiv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetSamplerParameterIuiv(sampler, pname, params) };
            _res
        }
        pub fn _glGetSamplerParameterfv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetSamplerParameterfv(sampler, pname, params) };
            _res
        }
        pub fn _glGetSamplerParameteriv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetSamplerParameteriv(sampler, pname, params) };
            _res
        }
        pub fn _glGetShaderInfoLog(
            _ctx: &mut Ctx,
            shader: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            infoLog: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let infoLog: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), infoLog) as *mut types::GLchar;
            let _res = unsafe { gl::GetShaderInfoLog(shader, bufSize, length, infoLog) };
            _res
        }
        pub fn _glGetShaderPrecisionFormat(
            _ctx: &mut Ctx,
            shadertype: types::GLenum,
            precisiontype: types::GLenum,
            range: u32,
            precision: u32,
        ) -> () {
            let range: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), range) as *mut types::GLint;
            let precision: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), precision) as *mut types::GLint;
            let _res = unsafe {
                gl::GetShaderPrecisionFormat(shadertype, precisiontype, range, precision)
            };
            _res
        }
        pub fn _glGetShaderSource(
            _ctx: &mut Ctx,
            shader: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            source: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let source: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), source) as *mut types::GLchar;
            let _res = unsafe { gl::GetShaderSource(shader, bufSize, length, source) };
            _res
        }
        pub fn _glGetShaderiv(
            _ctx: &mut Ctx,
            shader: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetShaderiv(shader, pname, params) };
            _res
        }
        pub fn _glGetString(_ctx: &mut Ctx, name: types::GLenum) -> u32 {
            let _res = unsafe { gl::GetString(name) };
            0
        }
        pub fn _glGetStringi(_ctx: &mut Ctx, name: types::GLenum, index: types::GLuint) -> u32 {
            let _res = unsafe { gl::GetStringi(name, index) };
            0
        }
        pub fn _glGetSubroutineIndex(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shadertype: types::GLenum,
            name: u32,
        ) -> types::GLuint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetSubroutineIndex(program, shadertype, name) };
            _res
        }
        pub fn _glGetSubroutineUniformLocation(
            _ctx: &mut Ctx,
            program: types::GLuint,
            shadertype: types::GLenum,
            name: u32,
        ) -> types::GLint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetSubroutineUniformLocation(program, shadertype, name) };
            _res
        }
        pub fn _glGetSynciv(
            _ctx: &mut Ctx,
            sync: u32,
            pname: types::GLenum,
            bufSize: types::GLsizei,
            length: u32,
            values: u32,
        ) -> () {
            let sync: types::GLsync =
                emscripten_memory_pointer!(_ctx.memory(0), sync) as types::GLsync;

            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let values: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), values) as *mut types::GLint;
            let _res = unsafe { gl::GetSynciv(sync, pname, bufSize, length, values) };
            _res
        }
        pub fn _glGetTexImage(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetTexImage(target, level, format, type_, pixels) };
            _res
        }
        pub fn _glGetTexLevelParameterfv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetTexLevelParameterfv(target, level, pname, params) };
            _res
        }
        pub fn _glGetTexLevelParameteriv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetTexLevelParameteriv(target, level, pname, params) };
            _res
        }
        pub fn _glGetTexParameterIiv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetTexParameterIiv(target, pname, params) };
            _res
        }
        pub fn _glGetTexParameterIuiv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetTexParameterIuiv(target, pname, params) };
            _res
        }
        pub fn _glGetTexParameterfv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetTexParameterfv(target, pname, params) };
            _res
        }
        pub fn _glGetTexParameteriv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetTexParameteriv(target, pname, params) };
            _res
        }
        pub fn _glGetTextureImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res =
                unsafe { gl::GetTextureImage(texture, level, format, type_, bufSize, pixels) };
            _res
        }
        pub fn _glGetTextureLevelParameterfv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetTextureLevelParameterfv(texture, level, pname, params) };
            _res
        }
        pub fn _glGetTextureLevelParameteriv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetTextureLevelParameteriv(texture, level, pname, params) };
            _res
        }
        pub fn _glGetTextureParameterIiv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetTextureParameterIiv(texture, pname, params) };
            _res
        }
        pub fn _glGetTextureParameterIuiv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetTextureParameterIuiv(texture, pname, params) };
            _res
        }
        pub fn _glGetTextureParameterfv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetTextureParameterfv(texture, pname, params) };
            _res
        }
        pub fn _glGetTextureParameteriv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetTextureParameteriv(texture, pname, params) };
            _res
        }
        pub fn _glGetTextureSubImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::GetTextureSubImage(
                    texture, level, xoffset, yoffset, zoffset, width, height, depth, format, type_,
                    bufSize, pixels,
                )
            };
            _res
        }
        pub fn _glGetTransformFeedbackVarying(
            _ctx: &mut Ctx,
            program: types::GLuint,
            index: types::GLuint,
            bufSize: types::GLsizei,
            length: u32,
            size: u32,
            type_: u32,
            name: u32,
        ) -> () {
            let length: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *mut types::GLsizei;
            let size: *mut types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), size) as *mut types::GLsizei;
            let type_: *mut types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), type_) as *mut types::GLenum;
            let name: *mut types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *mut types::GLchar;
            let _res = unsafe {
                gl::GetTransformFeedbackVarying(program, index, bufSize, length, size, type_, name)
            };
            _res
        }
        pub fn _glGetTransformFeedbacki64_v(
            _ctx: &mut Ctx,
            xfb: types::GLuint,
            pname: types::GLenum,
            index: types::GLuint,
            param: u32,
        ) -> () {
            let param: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *mut types::GLint64;
            let _res = unsafe { gl::GetTransformFeedbacki64_v(xfb, pname, index, param) };
            _res
        }
        pub fn _glGetTransformFeedbacki_v(
            _ctx: &mut Ctx,
            xfb: types::GLuint,
            pname: types::GLenum,
            index: types::GLuint,
            param: u32,
        ) -> () {
            let param: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *mut types::GLint;
            let _res = unsafe { gl::GetTransformFeedbacki_v(xfb, pname, index, param) };
            _res
        }
        pub fn _glGetTransformFeedbackiv(
            _ctx: &mut Ctx,
            xfb: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *mut types::GLint;
            let _res = unsafe { gl::GetTransformFeedbackiv(xfb, pname, param) };
            _res
        }
        pub fn _glGetUniformBlockIndex(
            _ctx: &mut Ctx,
            program: types::GLuint,
            uniformBlockName: u32,
        ) -> types::GLuint {
            let uniformBlockName: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), uniformBlockName)
                    as *const types::GLchar;
            let _res = unsafe { gl::GetUniformBlockIndex(program, uniformBlockName) };
            _res
        }
        pub fn _glGetUniformIndices(
            _ctx: &mut Ctx,
            program: types::GLuint,
            uniformCount: types::GLsizei,
            uniformNames: u32,
            uniformIndices: u32,
        ) -> () {
            let uniformNames: *const *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), uniformNames)
                    as *const *const types::GLchar;
            let uniformIndices: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), uniformIndices) as *mut types::GLuint;
            let _res = unsafe {
                gl::GetUniformIndices(program, uniformCount, uniformNames, uniformIndices)
            };
            _res
        }
        pub fn _glGetUniformLocation(
            _ctx: &mut Ctx,
            program: types::GLuint,
            name: u32,
        ) -> types::GLint {
            let name: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), name) as *const types::GLchar;
            let _res = unsafe { gl::GetUniformLocation(program, name) };
            _res
        }
        pub fn _glGetUniformSubroutineuiv(
            _ctx: &mut Ctx,
            shadertype: types::GLenum,
            location: types::GLint,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetUniformSubroutineuiv(shadertype, location, params) };
            _res
        }
        pub fn _glGetUniformdv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            params: u32,
        ) -> () {
            let params: *mut types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLdouble;
            let _res = unsafe { gl::GetUniformdv(program, location, params) };
            _res
        }
        pub fn _glGetUniformfv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetUniformfv(program, location, params) };
            _res
        }
        pub fn _glGetUniformiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetUniformiv(program, location, params) };
            _res
        }
        pub fn _glGetUniformuiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetUniformuiv(program, location, params) };
            _res
        }
        pub fn _glGetVertexArrayIndexed64iv(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            index: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *mut types::GLint64 =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *mut types::GLint64;
            let _res = unsafe { gl::GetVertexArrayIndexed64iv(vaobj, index, pname, param) };
            _res
        }
        pub fn _glGetVertexArrayIndexediv(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            index: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *mut types::GLint;
            let _res = unsafe { gl::GetVertexArrayIndexediv(vaobj, index, pname, param) };
            _res
        }
        pub fn _glGetVertexArrayiv(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *mut types::GLint;
            let _res = unsafe { gl::GetVertexArrayiv(vaobj, pname, param) };
            _res
        }
        pub fn _glGetVertexAttribIiv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetVertexAttribIiv(index, pname, params) };
            _res
        }
        pub fn _glGetVertexAttribIuiv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetVertexAttribIuiv(index, pname, params) };
            _res
        }
        pub fn _glGetVertexAttribLdv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLdouble;
            let _res = unsafe { gl::GetVertexAttribLdv(index, pname, params) };
            _res
        }
        pub fn _glGetVertexAttribPointerv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            pname: types::GLenum,
            pointer: u32,
        ) -> () {
            let pointer: *const *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pointer)
                    as *const *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetVertexAttribPointerv(index, pname, pointer) };
            _res
        }
        pub fn _glGetVertexAttribdv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLdouble;
            let _res = unsafe { gl::GetVertexAttribdv(index, pname, params) };
            _res
        }
        pub fn _glGetVertexAttribfv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetVertexAttribfv(index, pname, params) };
            _res
        }
        pub fn _glGetVertexAttribiv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetVertexAttribiv(index, pname, params) };
            _res
        }
        pub fn _glGetnColorTable(
            _ctx: &mut Ctx,
            target: types::GLenum,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            table: u32,
        ) -> () {
            let table: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), table) as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetnColorTable(target, format, type_, bufSize, table) };
            _res
        }
        pub fn _glGetnCompressedTexImage(
            _ctx: &mut Ctx,
            target: types::GLenum,
            lod: types::GLint,
            bufSize: types::GLsizei,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetnCompressedTexImage(target, lod, bufSize, pixels) };
            _res
        }
        pub fn _glGetnConvolutionFilter(
            _ctx: &mut Ctx,
            target: types::GLenum,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            image: u32,
        ) -> () {
            let image: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), image) as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetnConvolutionFilter(target, format, type_, bufSize, image) };
            _res
        }
        pub fn _glGetnHistogram(
            _ctx: &mut Ctx,
            target: types::GLenum,
            reset: types::GLboolean,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            values: u32,
        ) -> () {
            let values: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), values)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetnHistogram(target, reset, format, type_, bufSize, values) };
            _res
        }
        pub fn _glGetnMapdv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            query: types::GLenum,
            bufSize: types::GLsizei,
            v: u32,
        ) -> () {
            let v: *mut types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *mut types::GLdouble;
            let _res = unsafe { gl::GetnMapdv(target, query, bufSize, v) };
            _res
        }
        pub fn _glGetnMapfv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            query: types::GLenum,
            bufSize: types::GLsizei,
            v: u32,
        ) -> () {
            let v: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *mut types::GLfloat;
            let _res = unsafe { gl::GetnMapfv(target, query, bufSize, v) };
            _res
        }
        pub fn _glGetnMapiv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            query: types::GLenum,
            bufSize: types::GLsizei,
            v: u32,
        ) -> () {
            let v: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *mut types::GLint;
            let _res = unsafe { gl::GetnMapiv(target, query, bufSize, v) };
            _res
        }
        pub fn _glGetnMinmax(
            _ctx: &mut Ctx,
            target: types::GLenum,
            reset: types::GLboolean,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            values: u32,
        ) -> () {
            let values: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), values)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetnMinmax(target, reset, format, type_, bufSize, values) };
            _res
        }
        pub fn _glGetnPixelMapfv(
            _ctx: &mut Ctx,
            map: types::GLenum,
            bufSize: types::GLsizei,
            values: u32,
        ) -> () {
            let values: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), values) as *mut types::GLfloat;
            let _res = unsafe { gl::GetnPixelMapfv(map, bufSize, values) };
            _res
        }
        pub fn _glGetnPixelMapuiv(
            _ctx: &mut Ctx,
            map: types::GLenum,
            bufSize: types::GLsizei,
            values: u32,
        ) -> () {
            let values: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), values) as *mut types::GLuint;
            let _res = unsafe { gl::GetnPixelMapuiv(map, bufSize, values) };
            _res
        }
        pub fn _glGetnPixelMapusv(
            _ctx: &mut Ctx,
            map: types::GLenum,
            bufSize: types::GLsizei,
            values: u32,
        ) -> () {
            let values: *mut types::GLushort =
                emscripten_memory_pointer!(_ctx.memory(0), values) as *mut types::GLushort;
            let _res = unsafe { gl::GetnPixelMapusv(map, bufSize, values) };
            _res
        }
        pub fn _glGetnPolygonStipple(_ctx: &mut Ctx, bufSize: types::GLsizei, pattern: u32) -> () {
            let pattern: *mut types::GLubyte =
                emscripten_memory_pointer!(_ctx.memory(0), pattern) as *mut types::GLubyte;
            let _res = unsafe { gl::GetnPolygonStipple(bufSize, pattern) };
            _res
        }
        pub fn _glGetnSeparableFilter(
            _ctx: &mut Ctx,
            target: types::GLenum,
            format: types::GLenum,
            type_: types::GLenum,
            rowBufSize: types::GLsizei,
            row: u32,
            columnBufSize: types::GLsizei,
            column: u32,
            span: u32,
        ) -> () {
            let row: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), row) as *mut __gl_imports::raw::c_void;

            let column: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), column)
                    as *mut __gl_imports::raw::c_void;
            let span: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), span) as *mut __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::GetnSeparableFilter(
                    target,
                    format,
                    type_,
                    rowBufSize,
                    row,
                    columnBufSize,
                    column,
                    span,
                )
            };
            _res
        }
        pub fn _glGetnTexImage(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::GetnTexImage(target, level, format, type_, bufSize, pixels) };
            _res
        }
        pub fn _glGetnUniformdv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            bufSize: types::GLsizei,
            params: u32,
        ) -> () {
            let params: *mut types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLdouble;
            let _res = unsafe { gl::GetnUniformdv(program, location, bufSize, params) };
            _res
        }
        pub fn _glGetnUniformfv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            bufSize: types::GLsizei,
            params: u32,
        ) -> () {
            let params: *mut types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLfloat;
            let _res = unsafe { gl::GetnUniformfv(program, location, bufSize, params) };
            _res
        }
        pub fn _glGetnUniformiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            bufSize: types::GLsizei,
            params: u32,
        ) -> () {
            let params: *mut types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLint;
            let _res = unsafe { gl::GetnUniformiv(program, location, bufSize, params) };
            _res
        }
        pub fn _glGetnUniformuiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            bufSize: types::GLsizei,
            params: u32,
        ) -> () {
            let params: *mut types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *mut types::GLuint;
            let _res = unsafe { gl::GetnUniformuiv(program, location, bufSize, params) };
            _res
        }
        pub fn _glHint(_ctx: &mut Ctx, target: types::GLenum, mode: types::GLenum) -> () {
            let _res = unsafe { gl::Hint(target, mode) };
            _res
        }
        pub fn _glInvalidateBufferData(_ctx: &mut Ctx, buffer: types::GLuint) -> () {
            let _res = unsafe { gl::InvalidateBufferData(buffer) };
            _res
        }
        pub fn _glInvalidateBufferSubData(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            offset: u32,
            length: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let length: types::GLsizeiptr = length as types::GLsizeiptr;
            let _res = unsafe { gl::InvalidateBufferSubData(buffer, offset, length) };
            _res
        }
        pub fn _glInvalidateFramebuffer(
            _ctx: &mut Ctx,
            target: types::GLenum,
            numAttachments: types::GLsizei,
            attachments: u32,
        ) -> () {
            let attachments: *const types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), attachments) as *const types::GLenum;
            let _res = unsafe { gl::InvalidateFramebuffer(target, numAttachments, attachments) };
            _res
        }
        pub fn _glInvalidateNamedFramebufferData(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            numAttachments: types::GLsizei,
            attachments: u32,
        ) -> () {
            let attachments: *const types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), attachments) as *const types::GLenum;
            let _res = unsafe {
                gl::InvalidateNamedFramebufferData(framebuffer, numAttachments, attachments)
            };
            _res
        }
        pub fn _glInvalidateNamedFramebufferSubData(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            numAttachments: types::GLsizei,
            attachments: u32,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let attachments: *const types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), attachments) as *const types::GLenum;

            let _res = unsafe {
                gl::InvalidateNamedFramebufferSubData(
                    framebuffer,
                    numAttachments,
                    attachments,
                    x,
                    y,
                    width,
                    height,
                )
            };
            _res
        }
        pub fn _glInvalidateSubFramebuffer(
            _ctx: &mut Ctx,
            target: types::GLenum,
            numAttachments: types::GLsizei,
            attachments: u32,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let attachments: *const types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), attachments) as *const types::GLenum;

            let _res = unsafe {
                gl::InvalidateSubFramebuffer(
                    target,
                    numAttachments,
                    attachments,
                    x,
                    y,
                    width,
                    height,
                )
            };
            _res
        }
        pub fn _glInvalidateTexImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
        ) -> () {
            let _res = unsafe { gl::InvalidateTexImage(texture, level) };
            _res
        }
        pub fn _glInvalidateTexSubImage(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::InvalidateTexSubImage(
                    texture, level, xoffset, yoffset, zoffset, width, height, depth,
                )
            };
            _res
        }
        pub fn _glIsBuffer(_ctx: &mut Ctx, buffer: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsBuffer(buffer) };
            _res
        }
        pub fn _glIsEnabled(_ctx: &mut Ctx, cap: types::GLenum) -> types::GLboolean {
            let _res = unsafe { gl::IsEnabled(cap) };
            _res
        }
        pub fn _glIsEnabledi(
            _ctx: &mut Ctx,
            target: types::GLenum,
            index: types::GLuint,
        ) -> types::GLboolean {
            let _res = unsafe { gl::IsEnabledi(target, index) };
            _res
        }
        pub fn _glIsFramebuffer(_ctx: &mut Ctx, framebuffer: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsFramebuffer(framebuffer) };
            _res
        }
        pub fn _glIsProgram(_ctx: &mut Ctx, program: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsProgram(program) };
            _res
        }
        pub fn _glIsProgramPipeline(_ctx: &mut Ctx, pipeline: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsProgramPipeline(pipeline) };
            _res
        }
        pub fn _glIsQuery(_ctx: &mut Ctx, id: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsQuery(id) };
            _res
        }
        pub fn _glIsRenderbuffer(_ctx: &mut Ctx, renderbuffer: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsRenderbuffer(renderbuffer) };
            _res
        }
        pub fn _glIsSampler(_ctx: &mut Ctx, sampler: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsSampler(sampler) };
            _res
        }
        pub fn _glIsShader(_ctx: &mut Ctx, shader: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsShader(shader) };
            _res
        }
        pub fn _glIsSync(_ctx: &mut Ctx, sync: u32) -> types::GLboolean {
            let sync: types::GLsync =
                emscripten_memory_pointer!(_ctx.memory(0), sync) as types::GLsync;
            let _res = unsafe { gl::IsSync(sync) };
            _res
        }
        pub fn _glIsTexture(_ctx: &mut Ctx, texture: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsTexture(texture) };
            _res
        }
        pub fn _glIsTransformFeedback(_ctx: &mut Ctx, id: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsTransformFeedback(id) };
            _res
        }
        pub fn _glIsVertexArray(_ctx: &mut Ctx, array: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::IsVertexArray(array) };
            _res
        }
        pub fn _glLineWidth(_ctx: &mut Ctx, width: f64) -> () {
            let width: types::GLfloat = width as types::GLfloat;
            let _res = unsafe { gl::LineWidth(width) };
            _res
        }
        pub fn _glLinkProgram(_ctx: &mut Ctx, program: types::GLuint) -> () {
            let _res = unsafe { gl::LinkProgram(program) };
            _res
        }
        pub fn _glLogicOp(_ctx: &mut Ctx, opcode: types::GLenum) -> () {
            let _res = unsafe { gl::LogicOp(opcode) };
            _res
        }
        pub fn _glMapBuffer(_ctx: &mut Ctx, target: types::GLenum, access: types::GLenum) -> u32 {
            let _res = unsafe { gl::MapBuffer(target, access) };
            0
        }
        pub fn _glMapBufferRange(
            _ctx: &mut Ctx,
            target: types::GLenum,
            offset: u32,
            length: u32,
            access: types::GLbitfield,
        ) -> u32 {
            let offset: types::GLintptr = offset as types::GLintptr;
            let length: types::GLsizeiptr = length as types::GLsizeiptr;

            let _res = unsafe { gl::MapBufferRange(target, offset, length, access) };
            0
        }
        pub fn _glMapNamedBuffer(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            access: types::GLenum,
        ) -> u32 {
            let _res = unsafe { gl::MapNamedBuffer(buffer, access) };
            0
        }
        pub fn _glMapNamedBufferRange(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            offset: u32,
            length: u32,
            access: types::GLbitfield,
        ) -> u32 {
            let offset: types::GLintptr = offset as types::GLintptr;
            let length: types::GLsizeiptr = length as types::GLsizeiptr;

            let _res = unsafe { gl::MapNamedBufferRange(buffer, offset, length, access) };
            0
        }
        pub fn _glMemoryBarrier(_ctx: &mut Ctx, barriers: types::GLbitfield) -> () {
            let _res = unsafe { gl::MemoryBarrier(barriers) };
            _res
        }
        pub fn _glMemoryBarrierByRegion(_ctx: &mut Ctx, barriers: types::GLbitfield) -> () {
            let _res = unsafe { gl::MemoryBarrierByRegion(barriers) };
            _res
        }
        pub fn _glMinSampleShading(_ctx: &mut Ctx, value: f64) -> () {
            let value: types::GLfloat = value as types::GLfloat;
            let _res = unsafe { gl::MinSampleShading(value) };
            _res
        }
        pub fn _glMultiDrawArrays(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            first: u32,
            count: u32,
            drawcount: types::GLsizei,
        ) -> () {
            let first: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), first) as *const types::GLint;
            let count: *const types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), count) as *const types::GLsizei;

            let _res = unsafe { gl::MultiDrawArrays(mode, first, count, drawcount) };
            _res
        }
        pub fn _glMultiDrawArraysIndirect(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            indirect: u32,
            drawcount: types::GLsizei,
            stride: types::GLsizei,
        ) -> () {
            let indirect: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indirect)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::MultiDrawArraysIndirect(mode, indirect, drawcount, stride) };
            _res
        }
        pub fn _glMultiDrawElements(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: u32,
            type_: types::GLenum,
            indices: u32,
            drawcount: types::GLsizei,
        ) -> () {
            let count: *const types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), count) as *const types::GLsizei;

            let indices: *const *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::MultiDrawElements(mode, count, type_, indices, drawcount) };
            _res
        }
        pub fn _glMultiDrawElementsBaseVertex(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            count: u32,
            type_: types::GLenum,
            indices: u32,
            drawcount: types::GLsizei,
            basevertex: u32,
        ) -> () {
            let count: *const types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), count) as *const types::GLsizei;

            let indices: *const *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indices)
                    as *const *const __gl_imports::raw::c_void;

            let basevertex: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), basevertex) as *const types::GLint;
            let _res = unsafe {
                gl::MultiDrawElementsBaseVertex(mode, count, type_, indices, drawcount, basevertex)
            };
            _res
        }
        pub fn _glMultiDrawElementsIndirect(
            _ctx: &mut Ctx,
            mode: types::GLenum,
            type_: types::GLenum,
            indirect: u32,
            drawcount: types::GLsizei,
            stride: types::GLsizei,
        ) -> () {
            let indirect: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), indirect)
                    as *const __gl_imports::raw::c_void;

            let _res =
                unsafe { gl::MultiDrawElementsIndirect(mode, type_, indirect, drawcount, stride) };
            _res
        }
        pub fn _glMultiTexCoordP1ui(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::MultiTexCoordP1ui(texture, type_, coords) };
            _res
        }
        pub fn _glMultiTexCoordP1uiv(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: u32,
        ) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::MultiTexCoordP1uiv(texture, type_, coords) };
            _res
        }
        pub fn _glMultiTexCoordP2ui(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::MultiTexCoordP2ui(texture, type_, coords) };
            _res
        }
        pub fn _glMultiTexCoordP2uiv(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: u32,
        ) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::MultiTexCoordP2uiv(texture, type_, coords) };
            _res
        }
        pub fn _glMultiTexCoordP3ui(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::MultiTexCoordP3ui(texture, type_, coords) };
            _res
        }
        pub fn _glMultiTexCoordP3uiv(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: u32,
        ) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::MultiTexCoordP3uiv(texture, type_, coords) };
            _res
        }
        pub fn _glMultiTexCoordP4ui(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::MultiTexCoordP4ui(texture, type_, coords) };
            _res
        }
        pub fn _glMultiTexCoordP4uiv(
            _ctx: &mut Ctx,
            texture: types::GLenum,
            type_: types::GLenum,
            coords: u32,
        ) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::MultiTexCoordP4uiv(texture, type_, coords) };
            _res
        }
        pub fn _glNamedBufferData(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            size: u32,
            data: u32,
            usage: types::GLenum,
        ) -> () {
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::NamedBufferData(buffer, size, data, usage) };
            _res
        }
        pub fn _glNamedBufferStorage(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            size: u32,
            data: u32,
            flags: types::GLbitfield,
        ) -> () {
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::NamedBufferStorage(buffer, size, data, flags) };
            _res
        }
        pub fn _glNamedBufferSubData(
            _ctx: &mut Ctx,
            buffer: types::GLuint,
            offset: u32,
            size: u32,
            data: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let data: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::NamedBufferSubData(buffer, offset, size, data) };
            _res
        }
        pub fn _glNamedFramebufferDrawBuffer(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            buf: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::NamedFramebufferDrawBuffer(framebuffer, buf) };
            _res
        }
        pub fn _glNamedFramebufferDrawBuffers(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            n: types::GLsizei,
            bufs: u32,
        ) -> () {
            let bufs: *const types::GLenum =
                emscripten_memory_pointer!(_ctx.memory(0), bufs) as *const types::GLenum;
            let _res = unsafe { gl::NamedFramebufferDrawBuffers(framebuffer, n, bufs) };
            _res
        }
        pub fn _glNamedFramebufferParameteri(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            pname: types::GLenum,
            param: types::GLint,
        ) -> () {
            let _res = unsafe { gl::NamedFramebufferParameteri(framebuffer, pname, param) };
            _res
        }
        pub fn _glNamedFramebufferReadBuffer(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            src: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::NamedFramebufferReadBuffer(framebuffer, src) };
            _res
        }
        pub fn _glNamedFramebufferRenderbuffer(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            attachment: types::GLenum,
            renderbuffertarget: types::GLenum,
            renderbuffer: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::NamedFramebufferRenderbuffer(
                    framebuffer,
                    attachment,
                    renderbuffertarget,
                    renderbuffer,
                )
            };
            _res
        }
        pub fn _glNamedFramebufferTexture(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            attachment: types::GLenum,
            texture: types::GLuint,
            level: types::GLint,
        ) -> () {
            let _res =
                unsafe { gl::NamedFramebufferTexture(framebuffer, attachment, texture, level) };
            _res
        }
        pub fn _glNamedFramebufferTextureLayer(
            _ctx: &mut Ctx,
            framebuffer: types::GLuint,
            attachment: types::GLenum,
            texture: types::GLuint,
            level: types::GLint,
            layer: types::GLint,
        ) -> () {
            let _res = unsafe {
                gl::NamedFramebufferTextureLayer(framebuffer, attachment, texture, level, layer)
            };
            _res
        }
        pub fn _glNamedRenderbufferStorage(
            _ctx: &mut Ctx,
            renderbuffer: types::GLuint,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::NamedRenderbufferStorage(renderbuffer, internalformat, width, height)
            };
            _res
        }
        pub fn _glNamedRenderbufferStorageMultisample(
            _ctx: &mut Ctx,
            renderbuffer: types::GLuint,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::NamedRenderbufferStorageMultisample(
                    renderbuffer,
                    samples,
                    internalformat,
                    width,
                    height,
                )
            };
            _res
        }
        pub fn _glNormalP3ui(_ctx: &mut Ctx, type_: types::GLenum, coords: types::GLuint) -> () {
            let _res = unsafe { gl::NormalP3ui(type_, coords) };
            _res
        }
        pub fn _glNormalP3uiv(_ctx: &mut Ctx, type_: types::GLenum, coords: u32) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::NormalP3uiv(type_, coords) };
            _res
        }
        pub fn _glObjectLabel(
            _ctx: &mut Ctx,
            identifier: types::GLenum,
            name: types::GLuint,
            length: types::GLsizei,
            label: u32,
        ) -> () {
            let label: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), label) as *const types::GLchar;
            let _res = unsafe { gl::ObjectLabel(identifier, name, length, label) };
            _res
        }
        pub fn _glObjectPtrLabel(
            _ctx: &mut Ctx,
            ptr: u32,
            length: types::GLsizei,
            label: u32,
        ) -> () {
            let ptr: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), ptr) as *const __gl_imports::raw::c_void;

            let label: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), label) as *const types::GLchar;
            let _res = unsafe { gl::ObjectPtrLabel(ptr, length, label) };
            _res
        }
        pub fn _glPatchParameterfv(_ctx: &mut Ctx, pname: types::GLenum, values: u32) -> () {
            let values: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), values) as *const types::GLfloat;
            let _res = unsafe { gl::PatchParameterfv(pname, values) };
            _res
        }
        pub fn _glPatchParameteri(_ctx: &mut Ctx, pname: types::GLenum, value: types::GLint) -> () {
            let _res = unsafe { gl::PatchParameteri(pname, value) };
            _res
        }
        pub fn _glPauseTransformFeedback(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::PauseTransformFeedback() };
            _res
        }
        pub fn _glPixelStoref(_ctx: &mut Ctx, pname: types::GLenum, param: f64) -> () {
            let param: types::GLfloat = param as types::GLfloat;
            let _res = unsafe { gl::PixelStoref(pname, param) };
            _res
        }
        pub fn _glPixelStorei(_ctx: &mut Ctx, pname: types::GLenum, param: types::GLint) -> () {
            let _res = unsafe { gl::PixelStorei(pname, param) };
            _res
        }
        pub fn _glPointParameterf(_ctx: &mut Ctx, pname: types::GLenum, param: f64) -> () {
            let param: types::GLfloat = param as types::GLfloat;
            let _res = unsafe { gl::PointParameterf(pname, param) };
            _res
        }
        pub fn _glPointParameterfv(_ctx: &mut Ctx, pname: types::GLenum, params: u32) -> () {
            let params: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLfloat;
            let _res = unsafe { gl::PointParameterfv(pname, params) };
            _res
        }
        pub fn _glPointParameteri(_ctx: &mut Ctx, pname: types::GLenum, param: types::GLint) -> () {
            let _res = unsafe { gl::PointParameteri(pname, param) };
            _res
        }
        pub fn _glPointParameteriv(_ctx: &mut Ctx, pname: types::GLenum, params: u32) -> () {
            let params: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLint;
            let _res = unsafe { gl::PointParameteriv(pname, params) };
            _res
        }
        pub fn _glPointSize(_ctx: &mut Ctx, size: f64) -> () {
            let size: types::GLfloat = size as types::GLfloat;
            let _res = unsafe { gl::PointSize(size) };
            _res
        }
        pub fn _glPolygonMode(_ctx: &mut Ctx, face: types::GLenum, mode: types::GLenum) -> () {
            let _res = unsafe { gl::PolygonMode(face, mode) };
            _res
        }
        pub fn _glPolygonOffset(_ctx: &mut Ctx, factor: f64, units: f64) -> () {
            let factor: types::GLfloat = factor as types::GLfloat;
            let units: types::GLfloat = units as types::GLfloat;
            let _res = unsafe { gl::PolygonOffset(factor, units) };
            _res
        }
        pub fn _glPopDebugGroup(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::PopDebugGroup() };
            _res
        }
        pub fn _glPrimitiveRestartIndex(_ctx: &mut Ctx, index: types::GLuint) -> () {
            let _res = unsafe { gl::PrimitiveRestartIndex(index) };
            _res
        }
        pub fn _glProgramBinary(
            _ctx: &mut Ctx,
            program: types::GLuint,
            binaryFormat: types::GLenum,
            binary: u32,
            length: types::GLsizei,
        ) -> () {
            let binary: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), binary)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::ProgramBinary(program, binaryFormat, binary, length) };
            _res
        }
        pub fn _glProgramParameteri(
            _ctx: &mut Ctx,
            program: types::GLuint,
            pname: types::GLenum,
            value: types::GLint,
        ) -> () {
            let _res = unsafe { gl::ProgramParameteri(program, pname, value) };
            _res
        }
        pub fn _glProgramUniform1d(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform1d(program, location, v0) };
            _res
        }
        pub fn _glProgramUniform1dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::ProgramUniform1dv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform1f(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: f64,
        ) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let _res = unsafe { gl::ProgramUniform1f(program, location, v0) };
            _res
        }
        pub fn _glProgramUniform1fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::ProgramUniform1fv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform1i(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform1i(program, location, v0) };
            _res
        }
        pub fn _glProgramUniform1iv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::ProgramUniform1iv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform1ui(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform1ui(program, location, v0) };
            _res
        }
        pub fn _glProgramUniform1uiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::ProgramUniform1uiv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform2d(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLdouble,
            v1: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform2d(program, location, v0, v1) };
            _res
        }
        pub fn _glProgramUniform2dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::ProgramUniform2dv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform2f(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: f64,
            v1: f64,
        ) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let v1: types::GLfloat = v1 as types::GLfloat;
            let _res = unsafe { gl::ProgramUniform2f(program, location, v0, v1) };
            _res
        }
        pub fn _glProgramUniform2fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::ProgramUniform2fv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform2i(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLint,
            v1: types::GLint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform2i(program, location, v0, v1) };
            _res
        }
        pub fn _glProgramUniform2iv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::ProgramUniform2iv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform2ui(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLuint,
            v1: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform2ui(program, location, v0, v1) };
            _res
        }
        pub fn _glProgramUniform2uiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::ProgramUniform2uiv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform3d(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLdouble,
            v1: types::GLdouble,
            v2: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform3d(program, location, v0, v1, v2) };
            _res
        }
        pub fn _glProgramUniform3dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::ProgramUniform3dv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform3f(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: f64,
            v1: f64,
            v2: f64,
        ) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let v1: types::GLfloat = v1 as types::GLfloat;
            let v2: types::GLfloat = v2 as types::GLfloat;
            let _res = unsafe { gl::ProgramUniform3f(program, location, v0, v1, v2) };
            _res
        }
        pub fn _glProgramUniform3fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::ProgramUniform3fv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform3i(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLint,
            v1: types::GLint,
            v2: types::GLint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform3i(program, location, v0, v1, v2) };
            _res
        }
        pub fn _glProgramUniform3iv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::ProgramUniform3iv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform3ui(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLuint,
            v1: types::GLuint,
            v2: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform3ui(program, location, v0, v1, v2) };
            _res
        }
        pub fn _glProgramUniform3uiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::ProgramUniform3uiv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform4d(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLdouble,
            v1: types::GLdouble,
            v2: types::GLdouble,
            v3: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform4d(program, location, v0, v1, v2, v3) };
            _res
        }
        pub fn _glProgramUniform4dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::ProgramUniform4dv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform4f(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: f64,
            v1: f64,
            v2: f64,
            v3: f64,
        ) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let v1: types::GLfloat = v1 as types::GLfloat;
            let v2: types::GLfloat = v2 as types::GLfloat;
            let v3: types::GLfloat = v3 as types::GLfloat;
            let _res = unsafe { gl::ProgramUniform4f(program, location, v0, v1, v2, v3) };
            _res
        }
        pub fn _glProgramUniform4fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::ProgramUniform4fv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform4i(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLint,
            v1: types::GLint,
            v2: types::GLint,
            v3: types::GLint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform4i(program, location, v0, v1, v2, v3) };
            _res
        }
        pub fn _glProgramUniform4iv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::ProgramUniform4iv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniform4ui(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            v0: types::GLuint,
            v1: types::GLuint,
            v2: types::GLuint,
            v3: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::ProgramUniform4ui(program, location, v0, v1, v2, v3) };
            _res
        }
        pub fn _glProgramUniform4uiv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::ProgramUniform4uiv(program, location, count, value) };
            _res
        }
        pub fn _glProgramUniformMatrix2dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res =
                unsafe { gl::ProgramUniformMatrix2dv(program, location, count, transpose, value) };
            _res
        }
        pub fn _glProgramUniformMatrix2fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res =
                unsafe { gl::ProgramUniformMatrix2fv(program, location, count, transpose, value) };
            _res
        }
        pub fn _glProgramUniformMatrix2x3dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe {
                gl::ProgramUniformMatrix2x3dv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix2x3fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe {
                gl::ProgramUniformMatrix2x3fv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix2x4dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe {
                gl::ProgramUniformMatrix2x4dv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix2x4fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe {
                gl::ProgramUniformMatrix2x4fv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix3dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res =
                unsafe { gl::ProgramUniformMatrix3dv(program, location, count, transpose, value) };
            _res
        }
        pub fn _glProgramUniformMatrix3fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res =
                unsafe { gl::ProgramUniformMatrix3fv(program, location, count, transpose, value) };
            _res
        }
        pub fn _glProgramUniformMatrix3x2dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe {
                gl::ProgramUniformMatrix3x2dv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix3x2fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe {
                gl::ProgramUniformMatrix3x2fv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix3x4dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe {
                gl::ProgramUniformMatrix3x4dv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix3x4fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe {
                gl::ProgramUniformMatrix3x4fv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix4dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res =
                unsafe { gl::ProgramUniformMatrix4dv(program, location, count, transpose, value) };
            _res
        }
        pub fn _glProgramUniformMatrix4fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res =
                unsafe { gl::ProgramUniformMatrix4fv(program, location, count, transpose, value) };
            _res
        }
        pub fn _glProgramUniformMatrix4x2dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe {
                gl::ProgramUniformMatrix4x2dv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix4x2fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe {
                gl::ProgramUniformMatrix4x2fv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix4x3dv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe {
                gl::ProgramUniformMatrix4x3dv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProgramUniformMatrix4x3fv(
            _ctx: &mut Ctx,
            program: types::GLuint,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe {
                gl::ProgramUniformMatrix4x3fv(program, location, count, transpose, value)
            };
            _res
        }
        pub fn _glProvokingVertex(_ctx: &mut Ctx, mode: types::GLenum) -> () {
            let _res = unsafe { gl::ProvokingVertex(mode) };
            _res
        }
        pub fn _glPushDebugGroup(
            _ctx: &mut Ctx,
            source: types::GLenum,
            id: types::GLuint,
            length: types::GLsizei,
            message: u32,
        ) -> () {
            let message: *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), message) as *const types::GLchar;
            let _res = unsafe { gl::PushDebugGroup(source, id, length, message) };
            _res
        }
        pub fn _glQueryCounter(_ctx: &mut Ctx, id: types::GLuint, target: types::GLenum) -> () {
            let _res = unsafe { gl::QueryCounter(id, target) };
            _res
        }
        pub fn _glReadBuffer(_ctx: &mut Ctx, src: types::GLenum) -> () {
            let _res = unsafe { gl::ReadBuffer(src) };
            _res
        }
        pub fn _glReadPixels(
            _ctx: &mut Ctx,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *mut __gl_imports::raw::c_void;
            let _res = unsafe { gl::ReadPixels(x, y, width, height, format, type_, pixels) };
            _res
        }
        pub fn _glReadnPixels(
            _ctx: &mut Ctx,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            bufSize: types::GLsizei,
            data: u32,
        ) -> () {
            let data: *mut __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), data) as *mut __gl_imports::raw::c_void;
            let _res =
                unsafe { gl::ReadnPixels(x, y, width, height, format, type_, bufSize, data) };
            _res
        }
        pub fn _glReleaseShaderCompiler(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::ReleaseShaderCompiler() };
            _res
        }
        pub fn _glRenderbufferStorage(
            _ctx: &mut Ctx,
            target: types::GLenum,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::RenderbufferStorage(target, internalformat, width, height) };
            _res
        }
        pub fn _glRenderbufferStorageMultisample(
            _ctx: &mut Ctx,
            target: types::GLenum,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::RenderbufferStorageMultisample(target, samples, internalformat, width, height)
            };
            _res
        }
        pub fn _glResumeTransformFeedback(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::ResumeTransformFeedback() };
            _res
        }
        pub fn _glSampleCoverage(_ctx: &mut Ctx, value: f64, invert: types::GLboolean) -> () {
            let value: types::GLfloat = value as types::GLfloat;

            let _res = unsafe { gl::SampleCoverage(value, invert) };
            _res
        }
        pub fn _glSampleMaski(
            _ctx: &mut Ctx,
            maskNumber: types::GLuint,
            mask: types::GLbitfield,
        ) -> () {
            let _res = unsafe { gl::SampleMaski(maskNumber, mask) };
            _res
        }
        pub fn _glSamplerParameterIiv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *const types::GLint;
            let _res = unsafe { gl::SamplerParameterIiv(sampler, pname, param) };
            _res
        }
        pub fn _glSamplerParameterIuiv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *const types::GLuint;
            let _res = unsafe { gl::SamplerParameterIuiv(sampler, pname, param) };
            _res
        }
        pub fn _glSamplerParameterf(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            param: f64,
        ) -> () {
            let param: types::GLfloat = param as types::GLfloat;
            let _res = unsafe { gl::SamplerParameterf(sampler, pname, param) };
            _res
        }
        pub fn _glSamplerParameterfv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *const types::GLfloat;
            let _res = unsafe { gl::SamplerParameterfv(sampler, pname, param) };
            _res
        }
        pub fn _glSamplerParameteri(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            param: types::GLint,
        ) -> () {
            let _res = unsafe { gl::SamplerParameteri(sampler, pname, param) };
            _res
        }
        pub fn _glSamplerParameteriv(
            _ctx: &mut Ctx,
            sampler: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *const types::GLint;
            let _res = unsafe { gl::SamplerParameteriv(sampler, pname, param) };
            _res
        }
        pub fn _glScissor(
            _ctx: &mut Ctx,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::Scissor(x, y, width, height) };
            _res
        }
        pub fn _glScissorArrayv(
            _ctx: &mut Ctx,
            first: types::GLuint,
            count: types::GLsizei,
            v: u32,
        ) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::ScissorArrayv(first, count, v) };
            _res
        }
        pub fn _glScissorIndexed(
            _ctx: &mut Ctx,
            index: types::GLuint,
            left: types::GLint,
            bottom: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::ScissorIndexed(index, left, bottom, width, height) };
            _res
        }
        pub fn _glScissorIndexedv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::ScissorIndexedv(index, v) };
            _res
        }
        pub fn _glSecondaryColorP3ui(
            _ctx: &mut Ctx,
            type_: types::GLenum,
            color: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::SecondaryColorP3ui(type_, color) };
            _res
        }
        pub fn _glSecondaryColorP3uiv(_ctx: &mut Ctx, type_: types::GLenum, color: u32) -> () {
            let color: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), color) as *const types::GLuint;
            let _res = unsafe { gl::SecondaryColorP3uiv(type_, color) };
            _res
        }
        pub fn _glShaderBinary(
            _ctx: &mut Ctx,
            count: types::GLsizei,
            shaders: u32,
            binaryformat: types::GLenum,
            binary: u32,
            length: types::GLsizei,
        ) -> () {
            let shaders: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), shaders) as *const types::GLuint;

            let binary: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), binary)
                    as *const __gl_imports::raw::c_void;

            let _res = unsafe { gl::ShaderBinary(count, shaders, binaryformat, binary, length) };
            _res
        }
        pub fn _glShaderSource(
            _ctx: &mut Ctx,
            shader: types::GLuint,
            count: types::GLsizei,
            string: u32,
            length: u32,
        ) -> () {
            let string: *const *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), string) as *const *const types::GLchar;
            let length: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), length) as *const types::GLint;
            let _res = unsafe { gl::ShaderSource(shader, count, string, length) };
            _res
        }
        pub fn _glShaderStorageBlockBinding(
            _ctx: &mut Ctx,
            program: types::GLuint,
            storageBlockIndex: types::GLuint,
            storageBlockBinding: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::ShaderStorageBlockBinding(program, storageBlockIndex, storageBlockBinding)
            };
            _res
        }
        pub fn _glStencilFunc(
            _ctx: &mut Ctx,
            func: types::GLenum,
            ref_: types::GLint,
            mask: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::StencilFunc(func, ref_, mask) };
            _res
        }
        pub fn _glStencilFuncSeparate(
            _ctx: &mut Ctx,
            face: types::GLenum,
            func: types::GLenum,
            ref_: types::GLint,
            mask: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::StencilFuncSeparate(face, func, ref_, mask) };
            _res
        }
        pub fn _glStencilMask(_ctx: &mut Ctx, mask: types::GLuint) -> () {
            let _res = unsafe { gl::StencilMask(mask) };
            _res
        }
        pub fn _glStencilMaskSeparate(
            _ctx: &mut Ctx,
            face: types::GLenum,
            mask: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::StencilMaskSeparate(face, mask) };
            _res
        }
        pub fn _glStencilOp(
            _ctx: &mut Ctx,
            fail: types::GLenum,
            zfail: types::GLenum,
            zpass: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::StencilOp(fail, zfail, zpass) };
            _res
        }
        pub fn _glStencilOpSeparate(
            _ctx: &mut Ctx,
            face: types::GLenum,
            sfail: types::GLenum,
            dpfail: types::GLenum,
            dppass: types::GLenum,
        ) -> () {
            let _res = unsafe { gl::StencilOpSeparate(face, sfail, dpfail, dppass) };
            _res
        }
        pub fn _glTexBuffer(
            _ctx: &mut Ctx,
            target: types::GLenum,
            internalformat: types::GLenum,
            buffer: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::TexBuffer(target, internalformat, buffer) };
            _res
        }
        pub fn _glTexBufferRange(
            _ctx: &mut Ctx,
            target: types::GLenum,
            internalformat: types::GLenum,
            buffer: types::GLuint,
            offset: u32,
            size: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let _res = unsafe { gl::TexBufferRange(target, internalformat, buffer, offset, size) };
            _res
        }
        pub fn _glTexCoordP1ui(_ctx: &mut Ctx, type_: types::GLenum, coords: types::GLuint) -> () {
            let _res = unsafe { gl::TexCoordP1ui(type_, coords) };
            _res
        }
        pub fn _glTexCoordP1uiv(_ctx: &mut Ctx, type_: types::GLenum, coords: u32) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::TexCoordP1uiv(type_, coords) };
            _res
        }
        pub fn _glTexCoordP2ui(_ctx: &mut Ctx, type_: types::GLenum, coords: types::GLuint) -> () {
            let _res = unsafe { gl::TexCoordP2ui(type_, coords) };
            _res
        }
        pub fn _glTexCoordP2uiv(_ctx: &mut Ctx, type_: types::GLenum, coords: u32) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::TexCoordP2uiv(type_, coords) };
            _res
        }
        pub fn _glTexCoordP3ui(_ctx: &mut Ctx, type_: types::GLenum, coords: types::GLuint) -> () {
            let _res = unsafe { gl::TexCoordP3ui(type_, coords) };
            _res
        }
        pub fn _glTexCoordP3uiv(_ctx: &mut Ctx, type_: types::GLenum, coords: u32) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::TexCoordP3uiv(type_, coords) };
            _res
        }
        pub fn _glTexCoordP4ui(_ctx: &mut Ctx, type_: types::GLenum, coords: types::GLuint) -> () {
            let _res = unsafe { gl::TexCoordP4ui(type_, coords) };
            _res
        }
        pub fn _glTexCoordP4uiv(_ctx: &mut Ctx, type_: types::GLenum, coords: u32) -> () {
            let coords: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), coords) as *const types::GLuint;
            let _res = unsafe { gl::TexCoordP4uiv(type_, coords) };
            _res
        }
        pub fn _glTexImage1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLint,
            width: types::GLsizei,
            border: types::GLint,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TexImage1D(
                    target,
                    level,
                    internalformat,
                    width,
                    border,
                    format,
                    type_,
                    pixels,
                )
            };
            _res
        }
        pub fn _glTexImage2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            border: types::GLint,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TexImage2D(
                    target,
                    level,
                    internalformat,
                    width,
                    height,
                    border,
                    format,
                    type_,
                    pixels,
                )
            };
            _res
        }
        pub fn _glTexImage2DMultisample(
            _ctx: &mut Ctx,
            target: types::GLenum,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            fixedsamplelocations: types::GLboolean,
        ) -> () {
            let _res = unsafe {
                gl::TexImage2DMultisample(
                    target,
                    samples,
                    internalformat,
                    width,
                    height,
                    fixedsamplelocations,
                )
            };
            _res
        }
        pub fn _glTexImage3D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            internalformat: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            border: types::GLint,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TexImage3D(
                    target,
                    level,
                    internalformat,
                    width,
                    height,
                    depth,
                    border,
                    format,
                    type_,
                    pixels,
                )
            };
            _res
        }
        pub fn _glTexImage3DMultisample(
            _ctx: &mut Ctx,
            target: types::GLenum,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            fixedsamplelocations: types::GLboolean,
        ) -> () {
            let _res = unsafe {
                gl::TexImage3DMultisample(
                    target,
                    samples,
                    internalformat,
                    width,
                    height,
                    depth,
                    fixedsamplelocations,
                )
            };
            _res
        }
        pub fn _glTexParameterIiv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLint;
            let _res = unsafe { gl::TexParameterIiv(target, pname, params) };
            _res
        }
        pub fn _glTexParameterIuiv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLuint;
            let _res = unsafe { gl::TexParameterIuiv(target, pname, params) };
            _res
        }
        pub fn _glTexParameterf(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            param: f64,
        ) -> () {
            let param: types::GLfloat = param as types::GLfloat;
            let _res = unsafe { gl::TexParameterf(target, pname, param) };
            _res
        }
        pub fn _glTexParameterfv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLfloat;
            let _res = unsafe { gl::TexParameterfv(target, pname, params) };
            _res
        }
        pub fn _glTexParameteri(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            param: types::GLint,
        ) -> () {
            let _res = unsafe { gl::TexParameteri(target, pname, param) };
            _res
        }
        pub fn _glTexParameteriv(
            _ctx: &mut Ctx,
            target: types::GLenum,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLint;
            let _res = unsafe { gl::TexParameteriv(target, pname, params) };
            _res
        }
        pub fn _glTexStorage1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            levels: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::TexStorage1D(target, levels, internalformat, width) };
            _res
        }
        pub fn _glTexStorage2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            levels: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::TexStorage2D(target, levels, internalformat, width, height) };
            _res
        }
        pub fn _glTexStorage2DMultisample(
            _ctx: &mut Ctx,
            target: types::GLenum,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            fixedsamplelocations: types::GLboolean,
        ) -> () {
            let _res = unsafe {
                gl::TexStorage2DMultisample(
                    target,
                    samples,
                    internalformat,
                    width,
                    height,
                    fixedsamplelocations,
                )
            };
            _res
        }
        pub fn _glTexStorage3D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            levels: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
        ) -> () {
            let _res =
                unsafe { gl::TexStorage3D(target, levels, internalformat, width, height, depth) };
            _res
        }
        pub fn _glTexStorage3DMultisample(
            _ctx: &mut Ctx,
            target: types::GLenum,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            fixedsamplelocations: types::GLboolean,
        ) -> () {
            let _res = unsafe {
                gl::TexStorage3DMultisample(
                    target,
                    samples,
                    internalformat,
                    width,
                    height,
                    depth,
                    fixedsamplelocations,
                )
            };
            _res
        }
        pub fn _glTexSubImage1D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            width: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res =
                unsafe { gl::TexSubImage1D(target, level, xoffset, width, format, type_, pixels) };
            _res
        }
        pub fn _glTexSubImage2D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TexSubImage2D(
                    target, level, xoffset, yoffset, width, height, format, type_, pixels,
                )
            };
            _res
        }
        pub fn _glTexSubImage3D(
            _ctx: &mut Ctx,
            target: types::GLenum,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TexSubImage3D(
                    target, level, xoffset, yoffset, zoffset, width, height, depth, format, type_,
                    pixels,
                )
            };
            _res
        }
        pub fn _glTextureBarrier(_ctx: &mut Ctx) -> () {
            let _res = unsafe { gl::TextureBarrier() };
            _res
        }
        pub fn _glTextureBuffer(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            internalformat: types::GLenum,
            buffer: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::TextureBuffer(texture, internalformat, buffer) };
            _res
        }
        pub fn _glTextureBufferRange(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            internalformat: types::GLenum,
            buffer: types::GLuint,
            offset: u32,
            size: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let _res =
                unsafe { gl::TextureBufferRange(texture, internalformat, buffer, offset, size) };
            _res
        }
        pub fn _glTextureParameterIiv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLint;
            let _res = unsafe { gl::TextureParameterIiv(texture, pname, params) };
            _res
        }
        pub fn _glTextureParameterIuiv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            params: u32,
        ) -> () {
            let params: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), params) as *const types::GLuint;
            let _res = unsafe { gl::TextureParameterIuiv(texture, pname, params) };
            _res
        }
        pub fn _glTextureParameterf(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            param: f64,
        ) -> () {
            let param: types::GLfloat = param as types::GLfloat;
            let _res = unsafe { gl::TextureParameterf(texture, pname, param) };
            _res
        }
        pub fn _glTextureParameterfv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *const types::GLfloat;
            let _res = unsafe { gl::TextureParameterfv(texture, pname, param) };
            _res
        }
        pub fn _glTextureParameteri(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            param: types::GLint,
        ) -> () {
            let _res = unsafe { gl::TextureParameteri(texture, pname, param) };
            _res
        }
        pub fn _glTextureParameteriv(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            pname: types::GLenum,
            param: u32,
        ) -> () {
            let param: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), param) as *const types::GLint;
            let _res = unsafe { gl::TextureParameteriv(texture, pname, param) };
            _res
        }
        pub fn _glTextureStorage1D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            levels: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::TextureStorage1D(texture, levels, internalformat, width) };
            _res
        }
        pub fn _glTextureStorage2D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            levels: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res =
                unsafe { gl::TextureStorage2D(texture, levels, internalformat, width, height) };
            _res
        }
        pub fn _glTextureStorage2DMultisample(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            fixedsamplelocations: types::GLboolean,
        ) -> () {
            let _res = unsafe {
                gl::TextureStorage2DMultisample(
                    texture,
                    samples,
                    internalformat,
                    width,
                    height,
                    fixedsamplelocations,
                )
            };
            _res
        }
        pub fn _glTextureStorage3D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            levels: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
        ) -> () {
            let _res = unsafe {
                gl::TextureStorage3D(texture, levels, internalformat, width, height, depth)
            };
            _res
        }
        pub fn _glTextureStorage3DMultisample(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            samples: types::GLsizei,
            internalformat: types::GLenum,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            fixedsamplelocations: types::GLboolean,
        ) -> () {
            let _res = unsafe {
                gl::TextureStorage3DMultisample(
                    texture,
                    samples,
                    internalformat,
                    width,
                    height,
                    depth,
                    fixedsamplelocations,
                )
            };
            _res
        }
        pub fn _glTextureSubImage1D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            width: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TextureSubImage1D(texture, level, xoffset, width, format, type_, pixels)
            };
            _res
        }
        pub fn _glTextureSubImage2D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TextureSubImage2D(
                    texture, level, xoffset, yoffset, width, height, format, type_, pixels,
                )
            };
            _res
        }
        pub fn _glTextureSubImage3D(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            level: types::GLint,
            xoffset: types::GLint,
            yoffset: types::GLint,
            zoffset: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
            depth: types::GLsizei,
            format: types::GLenum,
            type_: types::GLenum,
            pixels: u32,
        ) -> () {
            let pixels: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pixels)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe {
                gl::TextureSubImage3D(
                    texture, level, xoffset, yoffset, zoffset, width, height, depth, format, type_,
                    pixels,
                )
            };
            _res
        }
        pub fn _glTextureView(
            _ctx: &mut Ctx,
            texture: types::GLuint,
            target: types::GLenum,
            origtexture: types::GLuint,
            internalformat: types::GLenum,
            minlevel: types::GLuint,
            numlevels: types::GLuint,
            minlayer: types::GLuint,
            numlayers: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::TextureView(
                    texture,
                    target,
                    origtexture,
                    internalformat,
                    minlevel,
                    numlevels,
                    minlayer,
                    numlayers,
                )
            };
            _res
        }
        pub fn _glTransformFeedbackBufferBase(
            _ctx: &mut Ctx,
            xfb: types::GLuint,
            index: types::GLuint,
            buffer: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::TransformFeedbackBufferBase(xfb, index, buffer) };
            _res
        }
        pub fn _glTransformFeedbackBufferRange(
            _ctx: &mut Ctx,
            xfb: types::GLuint,
            index: types::GLuint,
            buffer: types::GLuint,
            offset: u32,
            size: u32,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;
            let size: types::GLsizeiptr = size as types::GLsizeiptr;
            let _res =
                unsafe { gl::TransformFeedbackBufferRange(xfb, index, buffer, offset, size) };
            _res
        }
        pub fn _glTransformFeedbackVaryings(
            _ctx: &mut Ctx,
            program: types::GLuint,
            count: types::GLsizei,
            varyings: u32,
            bufferMode: types::GLenum,
        ) -> () {
            let varyings: *const *const types::GLchar =
                emscripten_memory_pointer!(_ctx.memory(0), varyings) as *const *const types::GLchar;

            let _res =
                unsafe { gl::TransformFeedbackVaryings(program, count, varyings, bufferMode) };
            _res
        }
        pub fn _glUniform1d(_ctx: &mut Ctx, location: types::GLint, x: types::GLdouble) -> () {
            let _res = unsafe { gl::Uniform1d(location, x) };
            _res
        }
        pub fn _glUniform1dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::Uniform1dv(location, count, value) };
            _res
        }
        pub fn _glUniform1f(_ctx: &mut Ctx, location: types::GLint, v0: f64) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let _res = unsafe { gl::Uniform1f(location, v0) };
            _res
        }
        pub fn _glUniform1fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::Uniform1fv(location, count, value) };
            _res
        }
        pub fn _glUniform1i(_ctx: &mut Ctx, location: types::GLint, v0: types::GLint) -> () {
            let _res = unsafe { gl::Uniform1i(location, v0) };
            _res
        }
        pub fn _glUniform1iv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::Uniform1iv(location, count, value) };
            _res
        }
        pub fn _glUniform1ui(_ctx: &mut Ctx, location: types::GLint, v0: types::GLuint) -> () {
            let _res = unsafe { gl::Uniform1ui(location, v0) };
            _res
        }
        pub fn _glUniform1uiv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::Uniform1uiv(location, count, value) };
            _res
        }
        pub fn _glUniform2d(
            _ctx: &mut Ctx,
            location: types::GLint,
            x: types::GLdouble,
            y: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::Uniform2d(location, x, y) };
            _res
        }
        pub fn _glUniform2dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::Uniform2dv(location, count, value) };
            _res
        }
        pub fn _glUniform2f(_ctx: &mut Ctx, location: types::GLint, v0: f64, v1: f64) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let v1: types::GLfloat = v1 as types::GLfloat;
            let _res = unsafe { gl::Uniform2f(location, v0, v1) };
            _res
        }
        pub fn _glUniform2fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::Uniform2fv(location, count, value) };
            _res
        }
        pub fn _glUniform2i(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: types::GLint,
            v1: types::GLint,
        ) -> () {
            let _res = unsafe { gl::Uniform2i(location, v0, v1) };
            _res
        }
        pub fn _glUniform2iv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::Uniform2iv(location, count, value) };
            _res
        }
        pub fn _glUniform2ui(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: types::GLuint,
            v1: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::Uniform2ui(location, v0, v1) };
            _res
        }
        pub fn _glUniform2uiv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::Uniform2uiv(location, count, value) };
            _res
        }
        pub fn _glUniform3d(
            _ctx: &mut Ctx,
            location: types::GLint,
            x: types::GLdouble,
            y: types::GLdouble,
            z: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::Uniform3d(location, x, y, z) };
            _res
        }
        pub fn _glUniform3dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::Uniform3dv(location, count, value) };
            _res
        }
        pub fn _glUniform3f(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: f64,
            v1: f64,
            v2: f64,
        ) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let v1: types::GLfloat = v1 as types::GLfloat;
            let v2: types::GLfloat = v2 as types::GLfloat;
            let _res = unsafe { gl::Uniform3f(location, v0, v1, v2) };
            _res
        }
        pub fn _glUniform3fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::Uniform3fv(location, count, value) };
            _res
        }
        pub fn _glUniform3i(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: types::GLint,
            v1: types::GLint,
            v2: types::GLint,
        ) -> () {
            let _res = unsafe { gl::Uniform3i(location, v0, v1, v2) };
            _res
        }
        pub fn _glUniform3iv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::Uniform3iv(location, count, value) };
            _res
        }
        pub fn _glUniform3ui(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: types::GLuint,
            v1: types::GLuint,
            v2: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::Uniform3ui(location, v0, v1, v2) };
            _res
        }
        pub fn _glUniform3uiv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::Uniform3uiv(location, count, value) };
            _res
        }
        pub fn _glUniform4d(
            _ctx: &mut Ctx,
            location: types::GLint,
            x: types::GLdouble,
            y: types::GLdouble,
            z: types::GLdouble,
            w: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::Uniform4d(location, x, y, z, w) };
            _res
        }
        pub fn _glUniform4dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::Uniform4dv(location, count, value) };
            _res
        }
        pub fn _glUniform4f(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: f64,
            v1: f64,
            v2: f64,
            v3: f64,
        ) -> () {
            let v0: types::GLfloat = v0 as types::GLfloat;
            let v1: types::GLfloat = v1 as types::GLfloat;
            let v2: types::GLfloat = v2 as types::GLfloat;
            let v3: types::GLfloat = v3 as types::GLfloat;
            let _res = unsafe { gl::Uniform4f(location, v0, v1, v2, v3) };
            _res
        }
        pub fn _glUniform4fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::Uniform4fv(location, count, value) };
            _res
        }
        pub fn _glUniform4i(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: types::GLint,
            v1: types::GLint,
            v2: types::GLint,
            v3: types::GLint,
        ) -> () {
            let _res = unsafe { gl::Uniform4i(location, v0, v1, v2, v3) };
            _res
        }
        pub fn _glUniform4iv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLint;
            let _res = unsafe { gl::Uniform4iv(location, count, value) };
            _res
        }
        pub fn _glUniform4ui(
            _ctx: &mut Ctx,
            location: types::GLint,
            v0: types::GLuint,
            v1: types::GLuint,
            v2: types::GLuint,
            v3: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::Uniform4ui(location, v0, v1, v2, v3) };
            _res
        }
        pub fn _glUniform4uiv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::Uniform4uiv(location, count, value) };
            _res
        }
        pub fn _glUniformBlockBinding(
            _ctx: &mut Ctx,
            program: types::GLuint,
            uniformBlockIndex: types::GLuint,
            uniformBlockBinding: types::GLuint,
        ) -> () {
            let _res =
                unsafe { gl::UniformBlockBinding(program, uniformBlockIndex, uniformBlockBinding) };
            _res
        }
        pub fn _glUniformMatrix2dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix2dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix2fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix2fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix2x3dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix2x3dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix2x3fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix2x3fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix2x4dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix2x4dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix2x4fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix2x4fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix3dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix3dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix3fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix3fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix3x2dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix3x2dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix3x2fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix3x2fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix3x4dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix3x4dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix3x4fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix3x4fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix4dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix4dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix4fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix4fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix4x2dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix4x2dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix4x2fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix4x2fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix4x3dv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLdouble;
            let _res = unsafe { gl::UniformMatrix4x3dv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformMatrix4x3fv(
            _ctx: &mut Ctx,
            location: types::GLint,
            count: types::GLsizei,
            transpose: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLfloat;
            let _res = unsafe { gl::UniformMatrix4x3fv(location, count, transpose, value) };
            _res
        }
        pub fn _glUniformSubroutinesuiv(
            _ctx: &mut Ctx,
            shadertype: types::GLenum,
            count: types::GLsizei,
            indices: u32,
        ) -> () {
            let indices: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), indices) as *const types::GLuint;
            let _res = unsafe { gl::UniformSubroutinesuiv(shadertype, count, indices) };
            _res
        }
        pub fn _glUnmapBuffer(_ctx: &mut Ctx, target: types::GLenum) -> types::GLboolean {
            let _res = unsafe { gl::UnmapBuffer(target) };
            _res
        }
        pub fn _glUnmapNamedBuffer(_ctx: &mut Ctx, buffer: types::GLuint) -> types::GLboolean {
            let _res = unsafe { gl::UnmapNamedBuffer(buffer) };
            _res
        }
        pub fn _glUseProgram(_ctx: &mut Ctx, program: types::GLuint) -> () {
            let _res = unsafe { gl::UseProgram(program) };
            _res
        }
        pub fn _glUseProgramStages(
            _ctx: &mut Ctx,
            pipeline: types::GLuint,
            stages: types::GLbitfield,
            program: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::UseProgramStages(pipeline, stages, program) };
            _res
        }
        pub fn _glValidateProgram(_ctx: &mut Ctx, program: types::GLuint) -> () {
            let _res = unsafe { gl::ValidateProgram(program) };
            _res
        }
        pub fn _glValidateProgramPipeline(_ctx: &mut Ctx, pipeline: types::GLuint) -> () {
            let _res = unsafe { gl::ValidateProgramPipeline(pipeline) };
            _res
        }
        pub fn _glVertexArrayAttribBinding(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            attribindex: types::GLuint,
            bindingindex: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexArrayAttribBinding(vaobj, attribindex, bindingindex) };
            _res
        }
        pub fn _glVertexArrayAttribFormat(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            attribindex: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            relativeoffset: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::VertexArrayAttribFormat(
                    vaobj,
                    attribindex,
                    size,
                    type_,
                    normalized,
                    relativeoffset,
                )
            };
            _res
        }
        pub fn _glVertexArrayAttribIFormat(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            attribindex: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            relativeoffset: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::VertexArrayAttribIFormat(vaobj, attribindex, size, type_, relativeoffset)
            };
            _res
        }
        pub fn _glVertexArrayAttribLFormat(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            attribindex: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            relativeoffset: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::VertexArrayAttribLFormat(vaobj, attribindex, size, type_, relativeoffset)
            };
            _res
        }
        pub fn _glVertexArrayBindingDivisor(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            bindingindex: types::GLuint,
            divisor: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexArrayBindingDivisor(vaobj, bindingindex, divisor) };
            _res
        }
        pub fn _glVertexArrayElementBuffer(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            buffer: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexArrayElementBuffer(vaobj, buffer) };
            _res
        }
        pub fn _glVertexArrayVertexBuffer(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            bindingindex: types::GLuint,
            buffer: types::GLuint,
            offset: u32,
            stride: types::GLsizei,
        ) -> () {
            let offset: types::GLintptr = offset as types::GLintptr;

            let _res =
                unsafe { gl::VertexArrayVertexBuffer(vaobj, bindingindex, buffer, offset, stride) };
            _res
        }
        pub fn _glVertexArrayVertexBuffers(
            _ctx: &mut Ctx,
            vaobj: types::GLuint,
            first: types::GLuint,
            count: types::GLsizei,
            buffers: u32,
            offsets: u32,
            strides: u32,
        ) -> () {
            let buffers: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), buffers) as *const types::GLuint;
            let offsets: *const types::GLintptr =
                emscripten_memory_pointer!(_ctx.memory(0), offsets) as *const types::GLintptr;
            let strides: *const types::GLsizei =
                emscripten_memory_pointer!(_ctx.memory(0), strides) as *const types::GLsizei;
            let _res = unsafe {
                gl::VertexArrayVertexBuffers(vaobj, first, count, buffers, offsets, strides)
            };
            _res
        }
        pub fn _glVertexAttrib1d(_ctx: &mut Ctx, index: types::GLuint, x: types::GLdouble) -> () {
            let _res = unsafe { gl::VertexAttrib1d(index, x) };
            _res
        }
        pub fn _glVertexAttrib1dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttrib1dv(index, v) };
            _res
        }
        pub fn _glVertexAttrib1f(_ctx: &mut Ctx, index: types::GLuint, x: f64) -> () {
            let x: types::GLfloat = x as types::GLfloat;
            let _res = unsafe { gl::VertexAttrib1f(index, x) };
            _res
        }
        pub fn _glVertexAttrib1fv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLfloat;
            let _res = unsafe { gl::VertexAttrib1fv(index, v) };
            _res
        }
        pub fn _glVertexAttrib1s(_ctx: &mut Ctx, index: types::GLuint, x: types::GLshort) -> () {
            let _res = unsafe { gl::VertexAttrib1s(index, x) };
            _res
        }
        pub fn _glVertexAttrib1sv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLshort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLshort;
            let _res = unsafe { gl::VertexAttrib1sv(index, v) };
            _res
        }
        pub fn _glVertexAttrib2d(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLdouble,
            y: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::VertexAttrib2d(index, x, y) };
            _res
        }
        pub fn _glVertexAttrib2dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttrib2dv(index, v) };
            _res
        }
        pub fn _glVertexAttrib2f(_ctx: &mut Ctx, index: types::GLuint, x: f64, y: f64) -> () {
            let x: types::GLfloat = x as types::GLfloat;
            let y: types::GLfloat = y as types::GLfloat;
            let _res = unsafe { gl::VertexAttrib2f(index, x, y) };
            _res
        }
        pub fn _glVertexAttrib2fv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLfloat;
            let _res = unsafe { gl::VertexAttrib2fv(index, v) };
            _res
        }
        pub fn _glVertexAttrib2s(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLshort,
            y: types::GLshort,
        ) -> () {
            let _res = unsafe { gl::VertexAttrib2s(index, x, y) };
            _res
        }
        pub fn _glVertexAttrib2sv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLshort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLshort;
            let _res = unsafe { gl::VertexAttrib2sv(index, v) };
            _res
        }
        pub fn _glVertexAttrib3d(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLdouble,
            y: types::GLdouble,
            z: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::VertexAttrib3d(index, x, y, z) };
            _res
        }
        pub fn _glVertexAttrib3dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttrib3dv(index, v) };
            _res
        }
        pub fn _glVertexAttrib3f(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: f64,
            y: f64,
            z: f64,
        ) -> () {
            let x: types::GLfloat = x as types::GLfloat;
            let y: types::GLfloat = y as types::GLfloat;
            let z: types::GLfloat = z as types::GLfloat;
            let _res = unsafe { gl::VertexAttrib3f(index, x, y, z) };
            _res
        }
        pub fn _glVertexAttrib3fv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLfloat;
            let _res = unsafe { gl::VertexAttrib3fv(index, v) };
            _res
        }
        pub fn _glVertexAttrib3s(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLshort,
            y: types::GLshort,
            z: types::GLshort,
        ) -> () {
            let _res = unsafe { gl::VertexAttrib3s(index, x, y, z) };
            _res
        }
        pub fn _glVertexAttrib3sv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLshort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLshort;
            let _res = unsafe { gl::VertexAttrib3sv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4Nbv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLbyte =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLbyte;
            let _res = unsafe { gl::VertexAttrib4Nbv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4Niv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::VertexAttrib4Niv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4Nsv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLshort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLshort;
            let _res = unsafe { gl::VertexAttrib4Nsv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4Nub(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLubyte,
            y: types::GLubyte,
            z: types::GLubyte,
            w: types::GLubyte,
        ) -> () {
            let _res = unsafe { gl::VertexAttrib4Nub(index, x, y, z, w) };
            _res
        }
        pub fn _glVertexAttrib4Nubv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLubyte =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLubyte;
            let _res = unsafe { gl::VertexAttrib4Nubv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4Nuiv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttrib4Nuiv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4Nusv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLushort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLushort;
            let _res = unsafe { gl::VertexAttrib4Nusv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4bv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLbyte =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLbyte;
            let _res = unsafe { gl::VertexAttrib4bv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4d(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLdouble,
            y: types::GLdouble,
            z: types::GLdouble,
            w: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::VertexAttrib4d(index, x, y, z, w) };
            _res
        }
        pub fn _glVertexAttrib4dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttrib4dv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4f(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: f64,
            y: f64,
            z: f64,
            w: f64,
        ) -> () {
            let x: types::GLfloat = x as types::GLfloat;
            let y: types::GLfloat = y as types::GLfloat;
            let z: types::GLfloat = z as types::GLfloat;
            let w: types::GLfloat = w as types::GLfloat;
            let _res = unsafe { gl::VertexAttrib4f(index, x, y, z, w) };
            _res
        }
        pub fn _glVertexAttrib4fv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLfloat;
            let _res = unsafe { gl::VertexAttrib4fv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4iv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::VertexAttrib4iv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4s(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLshort,
            y: types::GLshort,
            z: types::GLshort,
            w: types::GLshort,
        ) -> () {
            let _res = unsafe { gl::VertexAttrib4s(index, x, y, z, w) };
            _res
        }
        pub fn _glVertexAttrib4sv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLshort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLshort;
            let _res = unsafe { gl::VertexAttrib4sv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4ubv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLubyte =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLubyte;
            let _res = unsafe { gl::VertexAttrib4ubv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4uiv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttrib4uiv(index, v) };
            _res
        }
        pub fn _glVertexAttrib4usv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLushort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLushort;
            let _res = unsafe { gl::VertexAttrib4usv(index, v) };
            _res
        }
        pub fn _glVertexAttribBinding(
            _ctx: &mut Ctx,
            attribindex: types::GLuint,
            bindingindex: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribBinding(attribindex, bindingindex) };
            _res
        }
        pub fn _glVertexAttribDivisor(
            _ctx: &mut Ctx,
            index: types::GLuint,
            divisor: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribDivisor(index, divisor) };
            _res
        }
        pub fn _glVertexAttribFormat(
            _ctx: &mut Ctx,
            attribindex: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            relativeoffset: types::GLuint,
        ) -> () {
            let _res = unsafe {
                gl::VertexAttribFormat(attribindex, size, type_, normalized, relativeoffset)
            };
            _res
        }
        pub fn _glVertexAttribI1i(_ctx: &mut Ctx, index: types::GLuint, x: types::GLint) -> () {
            let _res = unsafe { gl::VertexAttribI1i(index, x) };
            _res
        }
        pub fn _glVertexAttribI1iv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::VertexAttribI1iv(index, v) };
            _res
        }
        pub fn _glVertexAttribI1ui(_ctx: &mut Ctx, index: types::GLuint, x: types::GLuint) -> () {
            let _res = unsafe { gl::VertexAttribI1ui(index, x) };
            _res
        }
        pub fn _glVertexAttribI1uiv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribI1uiv(index, v) };
            _res
        }
        pub fn _glVertexAttribI2i(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLint,
            y: types::GLint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribI2i(index, x, y) };
            _res
        }
        pub fn _glVertexAttribI2iv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::VertexAttribI2iv(index, v) };
            _res
        }
        pub fn _glVertexAttribI2ui(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLuint,
            y: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribI2ui(index, x, y) };
            _res
        }
        pub fn _glVertexAttribI2uiv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribI2uiv(index, v) };
            _res
        }
        pub fn _glVertexAttribI3i(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLint,
            y: types::GLint,
            z: types::GLint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribI3i(index, x, y, z) };
            _res
        }
        pub fn _glVertexAttribI3iv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::VertexAttribI3iv(index, v) };
            _res
        }
        pub fn _glVertexAttribI3ui(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLuint,
            y: types::GLuint,
            z: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribI3ui(index, x, y, z) };
            _res
        }
        pub fn _glVertexAttribI3uiv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribI3uiv(index, v) };
            _res
        }
        pub fn _glVertexAttribI4bv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLbyte =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLbyte;
            let _res = unsafe { gl::VertexAttribI4bv(index, v) };
            _res
        }
        pub fn _glVertexAttribI4i(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLint,
            y: types::GLint,
            z: types::GLint,
            w: types::GLint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribI4i(index, x, y, z, w) };
            _res
        }
        pub fn _glVertexAttribI4iv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLint;
            let _res = unsafe { gl::VertexAttribI4iv(index, v) };
            _res
        }
        pub fn _glVertexAttribI4sv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLshort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLshort;
            let _res = unsafe { gl::VertexAttribI4sv(index, v) };
            _res
        }
        pub fn _glVertexAttribI4ubv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLubyte =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLubyte;
            let _res = unsafe { gl::VertexAttribI4ubv(index, v) };
            _res
        }
        pub fn _glVertexAttribI4ui(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLuint,
            y: types::GLuint,
            z: types::GLuint,
            w: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribI4ui(index, x, y, z, w) };
            _res
        }
        pub fn _glVertexAttribI4uiv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribI4uiv(index, v) };
            _res
        }
        pub fn _glVertexAttribI4usv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLushort =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLushort;
            let _res = unsafe { gl::VertexAttribI4usv(index, v) };
            _res
        }
        pub fn _glVertexAttribIFormat(
            _ctx: &mut Ctx,
            attribindex: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            relativeoffset: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribIFormat(attribindex, size, type_, relativeoffset) };
            _res
        }
        pub fn _glVertexAttribIPointer(
            _ctx: &mut Ctx,
            index: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            stride: types::GLsizei,
            pointer: u32,
        ) -> () {
            let pointer: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pointer)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::VertexAttribIPointer(index, size, type_, stride, pointer) };
            _res
        }
        pub fn _glVertexAttribL1d(_ctx: &mut Ctx, index: types::GLuint, x: types::GLdouble) -> () {
            let _res = unsafe { gl::VertexAttribL1d(index, x) };
            _res
        }
        pub fn _glVertexAttribL1dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttribL1dv(index, v) };
            _res
        }
        pub fn _glVertexAttribL2d(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLdouble,
            y: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::VertexAttribL2d(index, x, y) };
            _res
        }
        pub fn _glVertexAttribL2dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttribL2dv(index, v) };
            _res
        }
        pub fn _glVertexAttribL3d(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLdouble,
            y: types::GLdouble,
            z: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::VertexAttribL3d(index, x, y, z) };
            _res
        }
        pub fn _glVertexAttribL3dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttribL3dv(index, v) };
            _res
        }
        pub fn _glVertexAttribL4d(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: types::GLdouble,
            y: types::GLdouble,
            z: types::GLdouble,
            w: types::GLdouble,
        ) -> () {
            let _res = unsafe { gl::VertexAttribL4d(index, x, y, z, w) };
            _res
        }
        pub fn _glVertexAttribL4dv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLdouble =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLdouble;
            let _res = unsafe { gl::VertexAttribL4dv(index, v) };
            _res
        }
        pub fn _glVertexAttribLFormat(
            _ctx: &mut Ctx,
            attribindex: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            relativeoffset: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribLFormat(attribindex, size, type_, relativeoffset) };
            _res
        }
        pub fn _glVertexAttribLPointer(
            _ctx: &mut Ctx,
            index: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            stride: types::GLsizei,
            pointer: u32,
        ) -> () {
            let pointer: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pointer)
                    as *const __gl_imports::raw::c_void;
            let _res = unsafe { gl::VertexAttribLPointer(index, size, type_, stride, pointer) };
            _res
        }
        pub fn _glVertexAttribP1ui(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribP1ui(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribP1uiv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribP1uiv(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribP2ui(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribP2ui(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribP2uiv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribP2uiv(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribP3ui(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribP3ui(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribP3uiv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribP3uiv(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribP4ui(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexAttribP4ui(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribP4uiv(
            _ctx: &mut Ctx,
            index: types::GLuint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            value: u32,
        ) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::VertexAttribP4uiv(index, type_, normalized, value) };
            _res
        }
        pub fn _glVertexAttribPointer(
            _ctx: &mut Ctx,
            index: types::GLuint,
            size: types::GLint,
            type_: types::GLenum,
            normalized: types::GLboolean,
            stride: types::GLsizei,
            pointer: u32,
        ) -> () {
            let pointer: *const __gl_imports::raw::c_void =
                emscripten_memory_pointer!(_ctx.memory(0), pointer)
                    as *const __gl_imports::raw::c_void;
            let _res =
                unsafe { gl::VertexAttribPointer(index, size, type_, normalized, stride, pointer) };
            _res
        }
        pub fn _glVertexBindingDivisor(
            _ctx: &mut Ctx,
            bindingindex: types::GLuint,
            divisor: types::GLuint,
        ) -> () {
            let _res = unsafe { gl::VertexBindingDivisor(bindingindex, divisor) };
            _res
        }
        pub fn _glVertexP2ui(_ctx: &mut Ctx, type_: types::GLenum, value: types::GLuint) -> () {
            let _res = unsafe { gl::VertexP2ui(type_, value) };
            _res
        }
        pub fn _glVertexP2uiv(_ctx: &mut Ctx, type_: types::GLenum, value: u32) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::VertexP2uiv(type_, value) };
            _res
        }
        pub fn _glVertexP3ui(_ctx: &mut Ctx, type_: types::GLenum, value: types::GLuint) -> () {
            let _res = unsafe { gl::VertexP3ui(type_, value) };
            _res
        }
        pub fn _glVertexP3uiv(_ctx: &mut Ctx, type_: types::GLenum, value: u32) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::VertexP3uiv(type_, value) };
            _res
        }
        pub fn _glVertexP4ui(_ctx: &mut Ctx, type_: types::GLenum, value: types::GLuint) -> () {
            let _res = unsafe { gl::VertexP4ui(type_, value) };
            _res
        }
        pub fn _glVertexP4uiv(_ctx: &mut Ctx, type_: types::GLenum, value: u32) -> () {
            let value: *const types::GLuint =
                emscripten_memory_pointer!(_ctx.memory(0), value) as *const types::GLuint;
            let _res = unsafe { gl::VertexP4uiv(type_, value) };
            _res
        }
        pub fn _glViewport(
            _ctx: &mut Ctx,
            x: types::GLint,
            y: types::GLint,
            width: types::GLsizei,
            height: types::GLsizei,
        ) -> () {
            let _res = unsafe { gl::Viewport(x, y, width, height) };
            _res
        }
        pub fn _glViewportArrayv(
            _ctx: &mut Ctx,
            first: types::GLuint,
            count: types::GLsizei,
            v: u32,
        ) -> () {
            let v: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLfloat;
            let _res = unsafe { gl::ViewportArrayv(first, count, v) };
            _res
        }
        pub fn _glViewportIndexedf(
            _ctx: &mut Ctx,
            index: types::GLuint,
            x: f64,
            y: f64,
            w: f64,
            h: f64,
        ) -> () {
            let x: types::GLfloat = x as types::GLfloat;
            let y: types::GLfloat = y as types::GLfloat;
            let w: types::GLfloat = w as types::GLfloat;
            let h: types::GLfloat = h as types::GLfloat;
            let _res = unsafe { gl::ViewportIndexedf(index, x, y, w, h) };
            _res
        }
        pub fn _glViewportIndexedfv(_ctx: &mut Ctx, index: types::GLuint, v: u32) -> () {
            let v: *const types::GLfloat =
                emscripten_memory_pointer!(_ctx.memory(0), v) as *const types::GLfloat;
            let _res = unsafe { gl::ViewportIndexedfv(index, v) };
            _res
        }
        pub fn _glWaitSync(
            _ctx: &mut Ctx,
            sync: u32,
            flags: types::GLbitfield,
            timeout: types::GLuint64,
        ) -> () {
            let sync: types::GLsync =
                emscripten_memory_pointer!(_ctx.memory(0), sync) as types::GLsync;

            let _res = unsafe { gl::WaitSync(sync, flags, timeout) };
            _res
        }
    }
}
