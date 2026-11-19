#include "terminal_shapes.h"
#include <QPainterPath>
#include <cmath>
#include <algorithm>

namespace {
// Same edge-weight catalogue as the Rune source renderer (up, right, down, left).
const char *boxLines = ".l.l .h.h l.l. h.h. .... .... .... .... .... .... .... .... .ll. .hl. .lh. .hh. ..ll ..lh ..hl ..hh ll.. lh.. hl.. hh.. l..l l..h h..l h..h lll. lhl. hll. llh. hlh. hhl. lhh. hhh. l.ll l.lh h.ll l.hl h.hl h.lh l.hh h.hh .lll .llh .hll .hlh .lhl .lhh .hhl .hhh ll.l ll.h lh.l lh.h hl.l hl.h hh.l hh.h llll lllh lhll lhlh hlll llhl hlhl hllh hhll llhh lhhl hhlh lhhh hlhh hhhl hhhh .... .... .... .... .d.d d.d. .dl. .ld. .dd. ..ld ..dl ..dd ld.. dl.. dd.. l..d d..l d..d ldl. dld. ddd. l.ld d.dl d.dd .dld .ldl .ddd ld.d dl.l dd.d ldld dldl dddd .... .... .... .... .... .... .... ...l l... .l.. ..l. ...h h... .h.. ..h. .h.l l.h. .l.h h.l.";
}

bool paintTerminalShape(QPainter &p, const QRectF &cell, const QString &text, const QColor &ink) {
    const auto chars=text.toUcs4();
    if(chars.size()!=1 || chars[0]<0x2500 || chars[0]>0x259f) return false;
    const uint c=chars[0];
    const auto transform=p.deviceTransform();
    const qreal scale=std::abs(transform.m11());
    if(scale<=0) return false;
    const qreal left=std::round(cell.left()*scale),top=std::round(cell.top()*scale);
    const qreal w=std::round(cell.right()*scale)-left,h=std::round(cell.bottom()*scale)-top;
    const qreal light=std::max(1.,std::round(scale));
    p.save();p.setRenderHint(QPainter::Antialiasing,false);p.translate(left/scale,top/scale);p.scale(1/scale,1/scale);
    const auto fill=[&](qreal x,qreal y,qreal width,qreal height){if(width>0 && height>0)p.fillRect(QRectF(x,y,width,height),ink);};
    const auto mid=[](qreal extent,qreal thick){return std::max(0.,std::floor((extent-thick)/2));};
    if(c>=0x2580) {
        const auto x=[&](qreal fraction){return std::round(w*fraction);};
        const auto y=[&](qreal fraction){return std::round(h*fraction);};
        if(c==0x2580) fill(0,0,w,y(.5));
        else if(c<=0x2587) {auto start=y(1-(c-0x2580)/8.);fill(0,start,w,h-start);}
        else if(c==0x2588) fill(0,0,w,h);
        else if(c<=0x258f) fill(0,0,x((0x2590-c)/8.),h);
        else if(c==0x2590) fill(x(.5),0,w-x(.5),h);
        else if(c<=0x2593) {p.setOpacity(p.opacity()*(c-0x2590)/4.);fill(0,0,w,h);}
        else if(c==0x2594) fill(0,0,w,y(1/8.));
        else if(c==0x2595) fill(x(7/8.),0,w-x(7/8.),h);
        else {
            const int bits[]={4,8,1,13,9,7,11,2,6,14};
            for(int i=0;i<4;++i) if(bits[c-0x2596]&(1<<i)) {
                auto x0=i%2?x(.5):0.,y0=i/2?y(.5):0.;
                fill(x0,y0,(i%2?w:x(.5))-x0,(i/2?h:y(.5))-y0);
            }
        }
    } else {
        const char *edges=boxLines+(c-0x2500)*5;
        if(c>=0x256d && c<=0x2570) {
            const int dx=(c==0x256d || c==0x2570)?1:-1,dy=c<=0x256e?1:-1;
            const qreal cx=mid(w,light)+light/2,cy=mid(h,light)+light/2;
            const qreal r=std::min({w/2,h/2,dx>0?w-cx:cx,dy>0?h-cy:cy});
            QPainterPath path;path.moveTo(cx,cy+dy*r);path.cubicTo(cx,cy+dy*.25*r,cx+dx*.25*r,cy,cx+dx*r,cy);
            p.setRenderHint(QPainter::Antialiasing,true);p.setPen(QPen(ink,light));p.drawPath(path);p.setRenderHint(QPainter::Antialiasing,false);
            const auto xx=std::round(cx+dx*.8*r),yy=std::round(cy+dy*.8*r);
            fill(mid(w,light),dy>0?yy:0,light,dy>0?h-yy:yy);
            fill(dx>0?xx:0,mid(h,light),dx>0?w-xx:xx,light);
        } else if(c>=0x2571 && c<=0x2573) {
            p.setRenderHint(QPainter::Antialiasing,true);p.setPen(QPen(ink,light));
            if(c!=0x2572)p.drawLine(QPointF(w,0),QPointF(0,h));
            if(c!=0x2571)p.drawLine(QPointF(0,0),QPointF(w,h));
        } else if((c>=0x2504 && c<=0x250b)||(c>=0x254c && c<=0x254f)) {
            const bool vertical=c>=0x254c?(c>=0x254e):((c-0x2504)%4>=2);
            const bool heavy=c%2;const qreal thick=heavy?2*light:light;
            const int count=c>=0x254c?2:c>=0x2508?4:3;
            const qreal along=vertical?h:w,across=vertical?w:h;
            const qreal gap=std::min(c>=0x254c?thick:std::max(light,4.),std::floor(along/(2*count)));
            const qreal total=std::floor(along)-count*gap,dash=std::floor(total/count);
            qreal extra=total-dash*count,at=vertical?0:std::floor(gap/2);
            for(int i=0;i<count;++i){qreal end=at+dash+(extra-->0?1:0);if(vertical)fill(mid(across,thick),at,thick,end-at);else fill(at,mid(across,thick),end-at,thick);at=end+gap;}
        } else {
            const char up=edges[0],right=edges[1],down=edges[2],leftEdge=edges[3];
            const qreal ht=mid(h,light),hb=ht+light,hht=mid(h,2*light),hhb=hht+2*light;
            const qreal hdt=std::max(0.,ht-light),hdb=hb+light;
            const qreal vl=mid(w,light),vr=vl+light,vhl=mid(w,2*light),vhr=vhl+2*light;
            const qreal vdl=std::max(0.,vl-light),vdr=vr+light;
            const qreal ub=(leftEdge=='h'||right=='h')?hhb:
                (leftEdge!=right||down==up)?((leftEdge=='d'||right=='d')?hdb:hb):
                (leftEdge=='.'&&right=='.')?hb:ht;
            const qreal dt=(leftEdge=='h'||right=='h')?hht:
                (leftEdge!=right||up==down)?((leftEdge=='d'||right=='d')?hdt:ht):
                (leftEdge=='.'&&right=='.')?ht:hb;
            const qreal lr=(up=='h'||down=='h')?vhr:
                (up!=down||leftEdge==right)?((up=='d'||down=='d')?vdr:vr):
                (up=='.'&&down=='.')?vr:vl;
            const qreal rl=(up=='h'||down=='h')?vhl:
                (up!=down||right==leftEdge)?((up=='d'||down=='d')?vdl:vl):
                (up=='.'&&down=='.')?vl:vr;
            const auto rect=[&](qreal x0,qreal y0,qreal x1,qreal y1){fill(x0,y0,x1-x0,y1-y0);};
            if(up=='l')rect(vl,0,vr,ub);
            if(up=='h')rect(vhl,0,vhr,ub);
            if(up=='d'){rect(vdl,0,vl,leftEdge=='d'?ht:ub);rect(vr,0,vdr,right=='d'?ht:ub);}
            if(right=='l')rect(rl,ht,w,hb);
            if(right=='h')rect(rl,hht,w,hhb);
            if(right=='d'){rect(up=='d'?vr:rl,hdt,w,ht);rect(down=='d'?vr:rl,hb,w,hdb);}
            if(down=='l')rect(vl,dt,vr,h);
            if(down=='h')rect(vhl,dt,vhr,h);
            if(down=='d'){rect(vdl,leftEdge=='d'?hb:dt,vl,h);rect(vr,right=='d'?hb:dt,vdr,h);}
            if(leftEdge=='l')rect(0,ht,lr,hb);
            if(leftEdge=='h')rect(0,hht,lr,hhb);
            if(leftEdge=='d'){rect(0,hdt,up=='d'?vl:lr,ht);rect(0,hb,down=='d'?vl:lr,hdb);}

        }
    }
    p.restore();return true;
}

void paintTerminalUnderline(QPainter &p,const QRectF &cell,const QString &style,const QColor &ink) {
    if(style.isEmpty() || style=="none")return;
    const qreal scale=std::abs(p.deviceTransform().m11());
    const qreal thickness=std::max(1.,std::round(scale))/scale;
    const qreal baseline=std::floor((cell.bottom()-2)*scale)/scale;
    p.save();p.setPen(QPen(ink,thickness,style=="dotted"?Qt::DotLine:style=="dashed"?Qt::DashLine:Qt::SolidLine));
    if(style=="curly") {
        QPainterPath wave;wave.moveTo(cell.left(),baseline);
        const qreal step=std::max(2.,cell.height()/6);
        for(qreal x=cell.left();x<cell.right();x+=step*2) {
            wave.cubicTo(x+step/2,baseline-step/2,x+step/2,baseline-step/2,x+step,baseline);
            wave.cubicTo(x+step*1.5,baseline+step/2,x+step*1.5,baseline+step/2,x+step*2,baseline);
        }
        p.setRenderHint(QPainter::Antialiasing,true);p.drawPath(wave);
    } else {
        p.drawLine(QPointF(cell.left(),baseline),QPointF(cell.right(),baseline));
        if(style=="double")p.drawLine(QPointF(cell.left(),baseline-2*thickness),QPointF(cell.right(),baseline-2*thickness));
    }
    p.restore();
}
