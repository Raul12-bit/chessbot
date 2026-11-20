"""Generate src/eval_params.rs with initial values: PeSTO material/PST + hand-picked guesses."""
import re
src = open('src/eval.rs', encoding='utf-8').read()
def table(name):
    body = src[src.index('const %s' % name):]
    body = body[body.index('= [') + 3:]; body = body[:body.index('];')]
    nums = [int(x) for x in re.findall(r'-?\d+', re.sub(r'//[^\n]*', '', body))]
    nums = [n for n in nums]
    assert len(nums) == 6 * 64 + 6 * 0, len(nums)
    return [nums[i*64:(i+1)*64] for i in range(6)]
mgt, egt = table('MG_TABLE'), table('EG_TABLE')
mgv = [82, 337, 365, 477, 1025, 0]; egv = [94, 281, 297, 512, 936, 0]
P = []
for p in range(6): P.append((mgv[p], egv[p]))
for p in range(6):
    for sq in range(64): P.append((mgt[p][sq ^ 56], egt[p][sq ^ 56]))
def mob(n, avg, kmg, keg): return [(int(kmg*(m-avg)), int(keg*(m-avg))) for m in range(n)]
P += mob(9, 4, 4, 4) + mob(14, 6, 5, 5) + mob(15, 7, 2, 4) + mob(28, 13, 1, 2)
P += [(0,0),(5,10),(5,15),(10,25),(25,50),(45,90),(70,140),(0,0)]  # passed by rank
P += [(-10,-20)]  # doubled
P += [(-10,-15)]  # isolated
P += [(30,50)]    # bishop pair
P += [(25,10)]    # rook open
P += [(10,5)]     # rook semi-open
P += [(6,0),(4,0),(6,0),(8,0)]  # king zone attacks N B R Q
P += [(12,0),(6,0)]  # shield
P += [(15,5)]     # tempo
assert len(P) == 476, len(P)
with open('src/eval_params.rs', 'w', newline='\n') as f:
    f.write('//! Evaluation parameters (mg, eg). Layout: see `evalt.rs`. Generated file.\n\n')
    f.write('#[rustfmt::skip]\npub const P: [(i32, i32); 476] = [\n')
    for i in range(0, len(P), 8):
        f.write('    ' + ' '.join('(%d, %d),' % t for t in P[i:i+8]) + '\n')
    f.write('];\n')
print('ok', len(P))
