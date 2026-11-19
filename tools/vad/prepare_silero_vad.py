"""Derive the VAD model bundled with the app from upstream Silero VAD.

tract (the pure Rust ONNX runtime used by the app) cannot analyse the `If`
nodes of the upstream model (it switches between 8 kHz and 16 kHz code paths
and on tensor shapes at run time). This script fixes the sample rate to
16 kHz, evaluates every `If` condition with onnxruntime for the fixed input
shapes ([1, 576] samples, state [2, 1, 128]), inlines the taken branch,
simplifies the graph and checks that the result is identical to the upstream
model on 200 random chunks.

Source model: silero_vad/data/silero_vad.onnx from the PyPI wheel
silero_vad-5.1.2-py3-none-any.whl
  sha256(wheel) = 93b41953d7774b165407fda6b533c119c5803864e367d5034dc626c82cfdf661
  sha256(onnx)  = 2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f
Output committed as crates/podcast-core/assets/silero_vad_16k.onnx
  sha256        = 861718705f2a3f36edc720f969263fcf465464a0cb5fc11571acecc4efce5524
(tested with onnx 1.23.2, onnxruntime 1.32.0, onnxsim 0.8.1)

Usage:
  python -m venv venv && venv/bin/pip install onnx onnxruntime onnxsim
  venv/bin/python -I prepare_silero_vad.py silero_vad.onnx silero_vad_16k.onnx
"""
import sys, copy, onnx, numpy as np, onnxruntime as ort
from onnx import helper
src, dst = sys.argv[1], sys.argv[2]
m = onnx.load(src)
g = m.graph
# fix sr to a constant 16000 and drop it as input
for i in list(g.input):
    if i.name == "sr": g.input.remove(i)
g.node.insert(0, helper.make_node("Constant", [], ["sr"], value=onnx.numpy_helper.from_array(np.array(16000, np.int64)), name="sr_const"))
feeds = {"input": np.zeros((1, 576), np.float32), "state": np.zeros((2, 1, 128), np.float32)}
for it in range(20):
    ifs = [n for n in g.node if n.op_type == "If"]
    if not ifs: break
    probe = copy.deepcopy(m)
    for n in ifs:
        probe.graph.output.append(helper.make_tensor_value_info(n.input[0], onnx.TensorProto.BOOL, None))
    sess = ort.InferenceSession(probe.SerializeToString())
    vals = sess.run([n.input[0] for n in ifs], feeds)
    new = []
    for n in g.node:
        if n.op_type != "If":
            new.append(n); continue
        cond = bool(np.asarray(vals[[x.name for x in ifs].index(n.name)]).item())
        br = [a.g for a in n.attribute if a.name == ("then_branch" if cond else "else_branch")][0]
        new.extend(br.node)
        g.initializer.extend(br.initializer)
        for bo, oo in zip(br.output, n.output):
            new.append(helper.make_node("Identity", [bo.name], [oo], name="inl_" + oo))
    del g.node[:]; g.node.extend(new)
    print("iteration", it, "inlined", len(ifs))
for i in g.input:
    if i.name == "input":
        d = i.type.tensor_type.shape.dim; d[0].dim_value = 1; d[1].dim_value = 576
    if i.name == "state":
        d = i.type.tensor_type.shape.dim; d[1].dim_value = 1
del g.value_info[:]
onnx.checker.check_model(m)
onnx.save(m, dst)
import onnxsim
ms, ok = onnxsim.simplify(onnx.load(dst))
assert ok
onnx.save(ms, dst)
print("ops:", sorted(set(n.op_type for n in ms.graph.node)))
a = ort.InferenceSession(src); b = ort.InferenceSession(dst)
rng = np.random.default_rng(0)
s1 = np.zeros((2,1,128), np.float32); s2 = s1.copy(); md = 0
for k in range(200):
    x = (np.sin(np.arange(576) * 0.05 * (1 + k % 7)) * 0.2 + rng.standard_normal(576) * 0.05).astype(np.float32)[None]
    o1, s1 = a.run(None, {"input": x, "state": s1, "sr": np.array(16000, np.int64)})
    o2, s2 = b.run(None, {"input": x, "state": s2})
    md = max(md, float(np.abs(o1 - o2).max()), float(np.abs(s1 - s2).max()))
print("max abs diff vs original:", md)
